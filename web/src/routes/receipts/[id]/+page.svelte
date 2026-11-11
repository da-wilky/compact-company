<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { formatDay, today } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Line = { description: string; account_id: number | null; net: string; vat_rate: string; tax: string };

	const id = $derived(Number(page.params.id));
	const owner = $derived(session.view?.user?.role === 'owner');

	let receipt = $state<Schemas['Receipt']>();
	let suppliers = $state<Schemas['Supplier'][]>([]);
	let accounts = $state<Schemas['Account'][]>([]);
	let currencies = $state<string[]>(['EUR']);
	let error = $state<ApiError>();
	let busy = $state(false);

	let supplierId = $state<number | null>(null);
	let number = $state('');
	let date = $state('');
	let dueDate = $state('');
	let currency = $state('EUR');
	let rate = $state('');
	let note = $state('');
	let lines = $state<Line[]>([]);

	/** Amounts may be negative, as on a credit note. */
	function signed(text: string, code: string): number | null {
		const trimmed = text.trim();
		const negative = trimmed.startsWith('-');
		const value = parseMoney(negative ? trimmed.slice(1) : trimmed, code);
		return value == null || Number.isNaN(value) ? value : negative ? -value : value;
	}
	const input = (minor: number, code: string) => (minor < 0 ? '-' : '') + moneyInput(Math.abs(minor), code);

	function show(data: Schemas['Receipt']) {
		receipt = data;
		supplierId = data.supplier_id ?? null;
		number = data.number ?? '';
		date = data.date ?? '';
		dueDate = data.due_date ?? '';
		currency = data.currency;
		rate = data.exchange_rate ?? '';
		note = data.note ?? '';
		lines = data.lines.map((l) => ({
			description: l.description,
			account_id: l.account_id ?? null,
			net: input(l.net, data.currency),
			vat_rate: l.vat_rate,
			tax: input(l.tax, data.currency)
		}));
	}

	async function load() {
		const result = await call(api.GET('/api/receipts/{id}', { params: { path: { id } } }));
		if (result.data) show(result.data);
		error = result.error;
	}

	$effect(() => {
		id;
		load();
	});
	call(api.GET('/api/suppliers')).then(({ data }) => (suppliers = data ?? []));
	call(api.GET('/api/accounts')).then(({ data }) => (accounts = data ?? []));
	call(api.GET('/api/currencies')).then(({ data }) => (currencies = data ?? ['EUR']));

	const expenseAccounts = $derived(accounts.filter((a) => a.type === 'expense' || a.type === 'asset'));
	const moneyAccounts = $derived(accounts.filter((a) => ['bank', 'cash', 'money_transit'].includes(a.role ?? '')));
	const accountName = (id: number | null | undefined) => {
		const account = accounts.find((a) => a.id === id);
		return account ? `${account.number} ${account.name}` : '–';
	};

	const totals = $derived.by(() => {
		let net = 0;
		let tax = 0;
		for (const line of lines) {
			const n = signed(line.net, currency) ?? 0;
			const r = Number(line.vat_rate.replace(',', '.')) || 0;
			const t = line.tax.trim() ? (signed(line.tax, currency) ?? 0) : Math.round((n * r) / 100);
			net += Number.isNaN(n) ? 0 : n;
			tax += Number.isNaN(t) ? 0 : t;
		}
		return { net, tax, gross: net + tax };
	});

	function body(): Schemas['ReceiptInput'] | undefined {
		const parsed = lines.map((l) => ({
			description: l.description,
			account_id: l.account_id,
			net: signed(l.net, currency),
			vat_rate: l.vat_rate.replace(',', '.').trim() || '0',
			tax: l.tax.trim() ? signed(l.tax, currency) : null
		}));
		const bad = parsed.findIndex((l) => l.net == null || Number.isNaN(l.net) || Number.isNaN(l.tax));
		if (bad >= 0) {
			error = { ...clientError('error.invalid_input'), fields: { [`lines.${bad}.net`]: 'invalid_format' } };
			return undefined;
		}
		return {
			supplier_id: supplierId,
			number: number || null,
			date: date || null,
			due_date: dueDate || null,
			currency,
			exchange_rate: currency === 'EUR' ? null : rate.replace(',', '.').trim() || null,
			note: note || null,
			lines: parsed as Schemas['ReceiptLineInput'][]
		};
	}

	async function save(event?: SubmitEvent) {
		event?.preventDefault();
		const input = body();
		if (!input) return false;
		busy = true;
		const result = await call(api.PUT('/api/receipts/{id}', { params: { path: { id } }, body: input }));
		busy = false;
		error = result.error;
		if (result.data) {
			show(result.data);
			if (event) toast(t('app.saved'));
		}
		return !result.error;
	}

	let paidFrom = $state<number | null>(null);
	async function book() {
		if (!(await save())) return;
		if (!confirm(t('receipts.book_confirm'))) return;
		busy = true;
		const result = await call(
			api.POST('/api/receipts/{id}/book', { params: { path: { id } }, body: { paid_from: paidFrom } })
		);
		busy = false;
		error = result.error;
		if (result.data) {
			show(result.data);
			toast(t('books.booked'));
		}
	}

	async function remove() {
		if (!confirm(t('receipts.delete_confirm'))) return;
		const result = await call(api.DELETE('/api/receipts/{id}', { params: { path: { id } } }));
		error = result.error;
		if (!result.error) goto('/receipts');
	}

	async function reverse() {
		if (!confirm(t('receipts.reverse_confirm'))) return;
		const result = await call(api.POST('/api/receipts/{id}/reverse', { params: { path: { id } } }));
		error = result.error;
		if (result.data) show(result.data);
	}

	// a supplier not known yet
	let supplierDialog = $state(false);
	let supplierName = $state('');
	let supplierError = $state<ApiError>();
	async function addSupplier(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(api.POST('/api/suppliers', { body: { name: supplierName } }));
		supplierError = result.error;
		if (result.data) {
			suppliers = [...suppliers, result.data].sort((a, b) => a.name.localeCompare(b.name));
			supplierId = result.data.id;
			supplierDialog = false;
		}
	}

	// payments of a booked receipt
	let payDate = $state(today());
	let payAmount = $state('');
	let payEur = $state('');
	let payAccount = $state<number>();
	let payError = $state<ApiError>();
	$effect(() => {
		payAccount ??= moneyAccounts.find((a) => a.role === 'bank')?.id;
	});

	async function pay(event: SubmitEvent) {
		event.preventDefault();
		if (!receipt) return;
		const amount = signed(payAmount, receipt.currency);
		const eur = payEur.trim() ? signed(payEur, 'EUR') : null;
		if (amount == null || Number.isNaN(amount) || Number.isNaN(eur)) {
			payError = { ...clientError('error.invalid_input'), fields: { amount: 'invalid_format' } };
			return;
		}
		const result = await call(
			api.POST('/api/receipts/{id}/payments', {
				params: { path: { id } },
				body: { date: payDate, amount, eur_amount: eur, account_id: payAccount ?? null }
			})
		);
		payError = result.error;
		if (result.data) {
			show(result.data);
			payAmount = '';
			payEur = '';
		}
	}

	async function unpay(paymentId: number) {
		if (!confirm(t('documents.payment_reverse_confirm'))) return;
		const result = await call(api.POST('/api/receipt-payments/{id}/reverse', { params: { path: { id: paymentId } } }));
		error = result.error;
		await load();
	}

	const file = $derived(receipt?.file_type ? `/api/receipts/${id}/file` : undefined);
	const draft = $derived(receipt?.status === 'draft' && owner);
	const day = (iso: string | null | undefined) => (iso ? formatDay(iso, { dateStyle: 'medium' }) : '–');
	const missing = (key: string) => (receipt?.missing?.[key] ? t(`field.${receipt.missing[key]}`) : undefined);
</script>

{#if receipt}
	<div class="page-head">
		<div>
			<p class="crumbs"><a href="/receipts">{t('receipts.title')}</a> · {t('term.receipt')}</p>
			<h1>{receipt.supplier_name ?? t('receipts.no_supplier')}{receipt.number ? ` · ${receipt.number}` : ''}</h1>
			<p class="muted">
				{t(receipt.status === 'draft' ? 'term.draft' : 'receipts.booked')}
				{#if receipt.format} · {t('receipts.read_from', { format: t(`receipts.format.${receipt.format}`) })}{/if}
				{#if receipt.reversed} · {t('books.is_reversed')}{/if}
			</p>
		</div>
		<div class="row">
			{#if file}<a class="button ghost" href={file} target="_blank" rel="noopener">{t('receipts.open_file')}</a>{/if}
			{#if draft}
				<button class="ghost" onclick={remove}>{t('app.delete')}</button>
			{:else if owner && receipt.status === 'booked' && !receipt.reversed}
				<button class="ghost" onclick={reverse}>{t('books.reverse')}</button>
			{/if}
		</div>
	</div>

	{#if receipt.duplicate_of}
		<p class="alert" role="status">
			{t('receipts.duplicate')} <a href="/receipts/{receipt.duplicate_of}">{t('receipts.show_duplicate')}</a>
		</p>
	{/if}
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	<div class="split" class:with-file={file && receipt.file_type !== 'application/xml'}>
		{#if file && receipt.file_type !== 'application/xml'}
			<div class="preview card">
				{#if receipt.file_type?.startsWith('image/')}
					<img src={file} alt={t('term.receipt')} />
				{:else}
					<iframe src={file} title={t('term.receipt')}></iframe>
				{/if}
			</div>
		{/if}

		<div class="stack">
			{#if draft}
				<form class="card stack" onsubmit={save}>
					<div class="grid">
						<Field label={t('term.supplier')} error={error?.fields?.supplier_id ?? receipt.missing.supplier_id}>
							<div class="with-button">
								<select bind:value={supplierId}>
									<option value={null}>{t('receipts.choose_supplier')}</option>
									{#each suppliers as supplier (supplier.id)}<option value={supplier.id}>{supplier.name}</option>{/each}
								</select>
								<button type="button" class="ghost icon" aria-label={t('suppliers.new')} onclick={() => { supplierName = ''; supplierError = undefined; supplierDialog = true; }}>
									<Icon name="plus" size={18} />
								</button>
							</div>
						</Field>
						<Field label={t('receipts.number')} error={error?.fields?.number}>
							<input bind:value={number} />
						</Field>
						<Field label={t('receipts.date')} error={error?.fields?.date ?? receipt.missing.date}>
							<input type="date" bind:value={date} />
						</Field>
						<Field label={t('documents.due_date')} error={error?.fields?.due_date}>
							<input type="date" bind:value={dueDate} />
						</Field>
						<Field label={t('customers.currency')} error={error?.fields?.currency}>
							<select bind:value={currency}>
								{#each currencies.includes(currency) ? currencies : [currency, ...currencies] as code (code)}<option value={code}>{code}</option>{/each}
							</select>
						</Field>
						{#if currency !== 'EUR'}
							<Field label={t('receipts.exchange_rate')} note={t('books.rate_note')} error={error?.fields?.exchange_rate ?? receipt.missing.exchange_rate}>
								<input inputmode="decimal" bind:value={rate} />
							</Field>
						{/if}
					</div>

					<fieldset class="stack">
						<legend>{t('books.lines')}</legend>
						{#each lines as line, i (i)}
							<div class="line">
								<input class="description" aria-label={t('documents.description')} placeholder={t('documents.description')} bind:value={line.description} />
								<select class="account" aria-label={t('term.ledger_account')} bind:value={line.account_id}>
									<option value={null}>{t('books.choose_account')}</option>
									{#each expenseAccounts as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
								</select>
								<input aria-label={t('receipts.net')} placeholder={t('receipts.net')} inputmode="decimal" bind:value={line.net} />
								<input class="rate" aria-label={t('receipts.vat_rate')} placeholder="%" inputmode="decimal" bind:value={line.vat_rate} />
								<input aria-label={t('receipts.tax')} placeholder={t('receipts.tax')} inputmode="decimal" bind:value={line.tax} />
								<button type="button" class="ghost icon" aria-label={t('app.remove')} onclick={() => lines.splice(i, 1)}>
									<Icon name="close" size={18} />
								</button>
								{#if error?.fields?.[`lines.${i}.net`] || error?.fields?.[`lines.${i}.description`] || missing(`lines.${i}.account_id`)}
									<small class="error wide">{t('receipts.line_incomplete')}</small>
								{/if}
							</div>
						{/each}
						<div>
							<button type="button" class="ghost" onclick={() => lines.push({ description: '', account_id: lines.at(-1)?.account_id ?? null, net: '', vat_rate: '19', tax: '' })}>
								<Icon name="plus" size={18} />{t('books.add_line')}
							</button>
						</div>
						{#if receipt.missing.lines}<small class="error">{t('receipts.needs_lines')}</small>{/if}
					</fieldset>

					<dl class="totals">
						<dt>{t('receipts.net')}</dt><dd>{formatMoney(totals.net, currency)}</dd>
						<dt>{t('receipts.tax')}</dt><dd>{formatMoney(totals.tax, currency)}</dd>
						<dt><strong>{t('receipts.gross')}</strong></dt><dd><strong>{formatMoney(totals.gross, currency)}</strong></dd>
					</dl>

					<Field label={t('receipts.note')} error={error?.fields?.note}>
						<textarea rows="2" bind:value={note}></textarea>
					</Field>

					<div class="row actions">
						<button class="ghost" disabled={busy}>{t('app.save')}</button>
						<label class="paid">
							<span class="muted">{t('receipts.paid_from')}</span>
							<select bind:value={paidFrom}>
								<option value={null}>{t('receipts.not_paid_yet')}</option>
								{#each moneyAccounts as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
							</select>
						</label>
						<button type="button" class="primary" disabled={busy} onclick={book}>{t('books.book')}</button>
					</div>
				</form>
			{:else}
				<section class="card stack">
					<dl class="facts">
						<dt>{t('receipts.date')}</dt><dd>{day(receipt.date)}</dd>
						<dt>{t('documents.due_date')}</dt><dd>{day(receipt.due_date)}</dd>
						{#if receipt.exchange_rate}<dt>{t('receipts.exchange_rate')}</dt><dd>{receipt.exchange_rate}</dd>{/if}
					</dl>
					<div class="table-wrap stacked">
						<table>
							<thead>
								<tr>
									<th>{t('documents.description')}</th>
									<th>{t('term.ledger_account')}</th>
									<th class="amount">{t('receipts.net')}</th>
									<th class="amount">{t('receipts.vat_rate')}</th>
									<th class="amount">{t('receipts.tax')}</th>
								</tr>
							</thead>
							<tbody>
								{#each receipt.lines as line (line.id)}
									<tr>
										<td data-label={t('documents.description')}>{line.description}</td>
										<td data-label={t('term.ledger_account')}>{accountName(line.account_id)}</td>
										<td data-label={t('receipts.net')} class="amount">{formatMoney(line.net, receipt.currency)}</td>
										<td data-label={t('receipts.vat_rate')} class="amount">{line.vat_rate} %</td>
										<td data-label={t('receipts.tax')} class="amount">{formatMoney(line.tax, receipt.currency)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
					<dl class="totals">
						<dt><strong>{t('receipts.gross')}</strong></dt><dd><strong>{formatMoney(receipt.gross, receipt.currency)}</strong></dd>
					</dl>
					{#if receipt.note}<p class="pre">{receipt.note}</p>{/if}
				</section>

				{#if receipt.amount_due != null && !receipt.reversed}
					<section class="card stack">
						<h2>{t('documents.payments')}</h2>
						<p>
							{t('documents.amount_due')}:
							<strong>{formatMoney(receipt.amount_due, receipt.currency)}</strong>
							{#if receipt.amount_due === 0}<small class="badge accent">{t('documents.paid')}</small>{/if}
						</p>
						{#if receipt.payments.length}
							<ul class="payments">
								{#each receipt.payments as payment (payment.id)}
									<li>
										<span>{day(payment.date)}</span>
										<span>{formatMoney(payment.amount, receipt.currency)}</span>
										{#if payment.eur_amount != null}<span class="muted">{formatMoney(payment.eur_amount, 'EUR')}</span>{/if}
										<span class="muted">{accountName(payment.account_id)}</span>
										{#if owner && payment.bank_match_id == null}<button class="ghost" onclick={() => unpay(payment.id)}>{t('books.reverse')}</button>{:else if payment.bank_match_id != null}<a href="/bank">{t('bank.title')}</a>{/if}
									</li>
								{/each}
							</ul>
						{/if}
						{#if owner && receipt.amount_due !== 0}
							<form class="pay" onsubmit={pay}>
								<Field label={t('receipts.paid_on')} error={payError?.fields?.date}>
									<input type="date" bind:value={payDate} />
								</Field>
								<Field label={t('documents.payment_amount')} error={payError?.fields?.amount} note={receipt.currency}>
									<input inputmode="decimal" bind:value={payAmount} placeholder={input(receipt.amount_due, receipt.currency)} />
								</Field>
								{#if receipt.currency !== 'EUR'}
									<Field label={t('receipts.left_in_eur')} error={payError?.fields?.eur_amount} note="EUR">
										<input inputmode="decimal" bind:value={payEur} />
									</Field>
								{/if}
								<Field label={t('documents.payment_account')} error={payError?.fields?.account_id}>
									<select bind:value={payAccount}>
										{#each moneyAccounts as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
									</select>
								</Field>
								<button class="primary">{t('receipts.record_payment')}</button>
							</form>
							{#if payError && !payError.fields}<p class="alert" role="alert">{t(payError.message)}</p>{/if}
						{/if}
					</section>
				{/if}
			{/if}
		</div>
	</div>

	<Dialog bind:open={supplierDialog} title={t('suppliers.new')}>
		<form class="stack" onsubmit={addSupplier}>
			<Field label={t('customers.name')} error={supplierError?.fields?.name}>
				<input bind:value={supplierName} />
			</Field>
			<p class="muted">{t('receipts.supplier_hint')}</p>
			<div><button class="primary">{t('app.save')}</button></div>
		</form>
	</Dialog>
{:else if error}
	<p class="alert" role="alert">{t(error.message)}</p>
{/if}

<style>
	.crumbs {
		margin: 0;
		color: var(--muted);
		font-size: 0.875rem;
	}
	.split {
		display: grid;
		gap: 1rem;
	}
	.split.with-file {
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
		align-items: start;
	}
	.preview {
		position: sticky;
		top: 1rem;
		padding: 0;
		overflow: hidden;
	}
	.preview iframe {
		display: block;
		width: 100%;
		height: min(80vh, 60rem);
		border: 0;
	}
	.preview img {
		display: block;
		width: 100%;
		height: auto;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
		gap: 0.75rem;
	}
	.with-button {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 0.25rem;
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		gap: 0.5rem;
	}
	legend {
		margin-bottom: 0.375rem;
		font-weight: 500;
	}
	/* two rows: what and where, then the amounts */
	.line {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 4.5rem minmax(0, 1fr) auto;
		gap: 0.375rem;
		padding-bottom: 0.5rem;
		border-bottom: 1px solid var(--border);
	}
	.line .description {
		grid-column: 1 / 3;
	}
	.line .account {
		grid-column: 3 / -1;
	}
	.line .wide {
		grid-column: 1 / -1;
	}
	.totals,
	.facts {
		display: grid;
		grid-template-columns: auto auto;
		gap: 0.25rem 1rem;
		margin: 0;
	}
	.totals {
		justify-content: end;
	}
	.facts {
		justify-content: start;
	}
	dd {
		margin: 0;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.facts dd {
		text-align: left;
	}
	.amount {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.actions {
		justify-content: end;
		flex-wrap: wrap;
	}
	.paid {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.paid select {
		width: auto;
	}
	.pay {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: end;
	}
	.payments {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 0.375rem;
	}
	.payments li {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: center;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
	.pre {
		white-space: pre-wrap;
	}
	@media (max-width: 64rem) {
		.split.with-file {
			grid-template-columns: 1fr;
		}
		.preview {
			position: static;
		}
		.preview iframe {
			height: 24rem;
		}
	}
	@media (max-width: 40rem) {
		.line .description,
		.line .account {
			grid-column: 1 / -1;
		}
		.preview img {
			max-height: 18rem;
			object-fit: contain;
		}
	}
</style>
