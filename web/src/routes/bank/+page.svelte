<script lang="ts">
	import { api, call, clientError, upload, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Part = {
		kind: Schemas['PartKind'];
		id: number | null;
		label: string;
		amount: string;
		currency: string;
		document_amount: string;
	};

	const owner = $derived(session.view?.user?.role === 'owner');
	const eur = (minor: number) => formatMoney(minor, 'EUR');
	const day = (iso: string) => formatDay(iso, { dateStyle: 'medium' });

	let books = $state<Schemas['BooksView']>();
	let banks = $state<Schemas['BankAccount'][]>([]);
	let bankId = $state<number>();
	let openOnly = $state(true);
	let transactions = $state<Schemas['BankTransaction'][]>([]);
	let accounts = $state<Schemas['Account'][]>([]);
	let error = $state<ApiError>();

	async function loadBanks() {
		const result = await call(api.GET('/api/bank-accounts'));
		banks = result.data ?? [];
		if (bankId == null || !banks.some((b) => b.id === bankId)) bankId = banks.find((b) => !b.archived)?.id;
	}

	async function loadTransactions() {
		if (bankId == null) {
			transactions = [];
			return;
		}
		const result = await call(
			api.GET('/api/bank-transactions', { params: { query: { bank_account_id: bankId, open: openOnly } } })
		);
		transactions = result.data ?? [];
		error = result.error;
	}

	call(api.GET('/api/books')).then(({ data }) => {
		books = data;
		if (data?.books) {
			loadBanks();
			call(api.GET('/api/accounts')).then(({ data }) => (accounts = data ?? []));
		}
	});
	$effect(() => {
		bankId;
		openOnly;
		loadTransactions();
	});

	const bank = $derived(banks.find((b) => b.id === bankId));

	// a new bank account
	let creating = $state(false);
	let name = $state('');
	let iban = $state('');
	let ledger = $state<number>();
	let createError = $state<ApiError>();
	const freeLedgers = $derived(
		accounts.filter((a) => a.type === 'asset' && !banks.some((b) => b.ledger_account_id === a.id))
	);

	async function newBank() {
		const business = await call(api.GET('/api/business'));
		name = banks.length ? '' : t('bank.default_name');
		iban = banks.length ? '' : (business.data?.values.iban ?? '');
		ledger = freeLedgers.find((a) => a.role === 'bank')?.id ?? freeLedgers[0]?.id;
		createError = undefined;
		creating = true;
	}

	async function createBank(event: SubmitEvent) {
		event.preventDefault();
		if (ledger == null) return;
		const result = await call(
			api.POST('/api/bank-accounts', { body: { name, iban: iban || null, ledger_account_id: ledger } })
		);
		createError = result.error;
		if (result.data) {
			creating = false;
			bankId = result.data.id;
			await loadBanks();
		}
	}

	// statements
	let fileInput = $state<HTMLInputElement>();
	let importing = $state(false);
	async function importFile(event: Event) {
		const file = (event.currentTarget as HTMLInputElement).files?.[0];
		(event.currentTarget as HTMLInputElement).value = '';
		if (!file || bankId == null) return;
		const form = new FormData();
		form.append('file', file);
		importing = true;
		const result = await upload<Schemas['ImportResult']>('POST', `/api/bank-accounts/${bankId}/import`, form);
		importing = false;
		error = result.error;
		if (result.data) {
			toast(t('bank.imported', { imported: String(result.data.imported), skipped: String(result.data.skipped) }));
			await Promise.all([loadBanks(), loadTransactions()]);
		}
	}

	// matching
	let detail = $state<Schemas['BankTransactionDetail']>();
	let parts = $state<Part[]>([]);
	let matchError = $state<ApiError>();
	let matchOpen = $state(false);

	const sign = (n: number) => (n < 0 ? '-' : '');
	const input = (minor: number, currency = 'EUR') => sign(minor) + moneyInput(Math.abs(minor), currency);
	function signed(text: string, currency = 'EUR'): number | null {
		const trimmed = text.trim();
		const negative = trimmed.startsWith('-');
		const value = parseMoney(negative ? trimmed.slice(1) : trimmed, currency);
		return value == null || Number.isNaN(value) ? value : negative ? -value : value;
	}

	async function openMatch(transaction: Schemas['BankTransaction']) {
		const result = await call(api.GET('/api/bank-transactions/{id}', { params: { path: { id: transaction.id } } }));
		if (!result.data) return;
		detail = result.data;
		matchError = undefined;
		const best = detail.candidates[0];
		parts = best && best.score >= 50 ? [candidatePart(best)] : [];
		matchOpen = true;
	}

	function candidatePart(c: Schemas['Candidate']): Part {
		return {
			kind: c.kind,
			id: c.id,
			label: c.label,
			amount: input(c.amount),
			currency: c.currency,
			document_amount: c.currency === 'EUR' ? '' : input(c.due, c.currency)
		};
	}

	const assigned = $derived(parts.reduce((sum, p) => sum + (signed(p.amount) || 0), 0));
	const rest = $derived((detail?.amount ?? 0) - assigned);
	const chosen = (c: Schemas['Candidate']) => parts.some((p) => p.kind === c.kind && p.id === c.id);

	function toggle(c: Schemas['Candidate']) {
		const i = parts.findIndex((p) => p.kind === c.kind && p.id === c.id);
		if (i >= 0) parts.splice(i, 1);
		else {
			const part = candidatePart(c);
			// never more than what is left of the transaction
			const amount = signed(part.amount) ?? 0;
			if (Math.abs(amount) > Math.abs(rest) && rest !== 0 && Math.sign(rest) === Math.sign(amount)) part.amount = input(rest);
			parts.push(part);
		}
	}

	function addAccount(id: number | null = null) {
		parts.push({ kind: 'account', id, label: '', amount: input(rest), currency: 'EUR', document_amount: '' });
	}

	// the accounts a transaction without an invoice or receipt usually goes to
	const quick = $derived(
		(detail && detail.amount < 0 ? ['bank_fees', 'private_withdrawals', 'vat_prepayments'] : ['private_deposits'])
			.map((role) => accounts.find((a) => a.role === role))
			.filter((a) => a != null)
	);

	async function match(event: SubmitEvent) {
		event.preventDefault();
		if (!detail) return;
		const body = parts.map((p) => ({
			kind: p.kind,
			id: p.id ?? 0,
			amount: signed(p.amount),
			document_amount: p.document_amount.trim() ? signed(p.document_amount, p.currency) : null
		}));
		if (body.some((p) => p.amount == null || Number.isNaN(p.amount) || !p.id || Number.isNaN(p.document_amount))) {
			matchError = { ...clientError('error.invalid_input'), fields: { parts: 'invalid_format' } };
			return;
		}
		const result = await call(
			api.POST('/api/bank-transactions/{id}/match', {
				params: { path: { id: detail.id } },
				body: { parts: body as Schemas['MatchPartInput'][] }
			})
		);
		matchError = result.error;
		if (result.data) {
			matchOpen = false;
			toast(t('bank.matched'));
			await Promise.all([loadBanks(), loadTransactions()]);
		}
	}

	async function unmatch(transaction: Schemas['BankTransaction']) {
		if (!confirm(t('bank.unmatch_confirm'))) return;
		const result = await call(api.POST('/api/bank-transactions/{id}/unmatch', { params: { path: { id: transaction.id } } }));
		error = result.error;
		await Promise.all([loadBanks(), loadTransactions()]);
	}

	const bookable = $derived(accounts.filter((a) => !a.archived && !banks.some((b) => b.ledger_account_id === a.id)));
	const href = (part: Schemas['MatchPart']) =>
		part.kind === 'invoice' ? `/documents/${part.id}` : part.kind === 'receipt' ? `/receipts/${part.id}` : undefined;
</script>

<div class="page-head">
	<div>
		<h1>{t('bank.title')}</h1>
		<p class="muted">{t('bank.intro')}</p>
	</div>
	{#if owner && books?.books}
		<div class="row">
			<button class="ghost" onclick={newBank}><Icon name="plus" size={18} />{t('bank.new_account')}</button>
			{#if bank}
				<button class="primary" disabled={importing} onclick={() => fileInput?.click()}>{t('bank.import')}</button>
				<input bind:this={fileInput} type="file" hidden accept=".xml,.csv,.txt,text/csv,application/xml,text/xml" onchange={importFile} />
			{/if}
		</div>
	{/if}
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}{error.detail ? ` (${error.detail})` : ''}</p>{/if}

{#if books && !books.books}
	<p class="card empty">{t('error.books_not_started')} <a href="/books">{t('books.title')}</a></p>
{:else if books}
	<div class="stack">
		{#if banks.length}
			<div class="row">
				{#if banks.length > 1}
					<select class="auto" aria-label={t('bank.account')} bind:value={bankId}>
						{#each banks as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
					</select>
				{/if}
				<Segmented
					label={t('bank.title')}
					options={[
						{ value: 'open', label: t('bank.open') },
						{ value: 'all', label: t('receipts.all') }
					]}
					value={openOnly ? 'open' : 'all'}
					onchange={(v) => (openOnly = v === 'open')}
				/>
				{#if bank}
					<p class="muted summary">
						{bank.iban ?? bank.ledger_account} · {t('books.balance')} {eur(bank.balance)} · {t('bank.open_count', { count: String(bank.open) })}
					</p>
				{/if}
			</div>
			{#if transactions.length}
				<ul class="transactions">
					{#each transactions as transaction (transaction.id)}
						<li class="card">
							<div class="head">
								<span class="date">{day(transaction.booking_date)}</span>
								<strong class="party">{transaction.counterparty ?? t('bank.no_counterparty')}</strong>
								<span class="amount" class:in={transaction.amount > 0}>{eur(transaction.amount)}</span>
							</div>
							{#if transaction.purpose}<p class="purpose">{transaction.purpose}</p>{/if}
							{#if transaction.match_id != null}
								<div class="parts">
									{#each transaction.parts as part, i (i)}
										<span class="badge accent">
											{#if href(part)}<a href={href(part)}>{part.label}</a>{:else}{part.label}{/if}
											{eur(part.amount)}
										</span>
									{/each}
									{#if owner}<button class="ghost" onclick={() => unmatch(transaction)}>{t('bank.unmatch')}</button>{/if}
								</div>
							{:else if owner}
								<div><button class="ghost" onclick={() => openMatch(transaction)}>{t('bank.match')}</button></div>
							{/if}
						</li>
					{/each}
				</ul>
			{:else}
				<p class="card empty">{t(openOnly ? 'bank.none_open' : 'bank.none')}</p>
			{/if}
		{:else}
			<p class="card empty">{t('bank.no_accounts')}</p>
		{/if}
	</div>
{/if}

<Dialog bind:open={creating} title={t('bank.new_account')}>
	<form class="stack" onsubmit={createBank}>
		<Field label={t('customers.name')} error={createError?.fields?.name}><input bind:value={name} /></Field>
		<Field label={t('business.iban')} error={createError?.fields?.iban}><input bind:value={iban} /></Field>
		<Field label={t('term.ledger_account')} error={createError?.fields?.ledger_account_id}>
			<select bind:value={ledger}>
				{#each freeLedgers as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
			</select>
		</Field>
		{#if createError}<p class="alert" role="alert">{t(createError.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<Dialog bind:open={matchOpen} title={t('bank.match')}>
	{#if detail}
		<form class="stack" onsubmit={match}>
			<div class="summary-box">
				<strong>{eur(detail.amount)}</strong>
				<span>{day(detail.booking_date)} · {detail.counterparty ?? t('bank.no_counterparty')}</span>
				{#if detail.purpose}<small class="muted">{detail.purpose}</small>{/if}
			</div>

			{#if detail.candidates.length}
				<fieldset class="stack">
					<legend>{t(detail.amount > 0 ? 'bank.open_invoices' : 'bank.open_receipts')}</legend>
					<ul class="candidates">
						{#each detail.candidates.slice(0, 8) as candidate (`${candidate.kind}${candidate.id}`)}
							<li>
								<label>
									<input type="checkbox" checked={chosen(candidate)} onchange={() => toggle(candidate)} />
									<span class="label">{candidate.label}</span>
									<span class="muted">{formatMoney(candidate.due, candidate.currency)}</span>
									{#if candidate.score >= 50}<span class="badge accent">{t('bank.likely')}</span>{/if}
								</label>
							</li>
						{/each}
					</ul>
				</fieldset>
			{/if}

			{#if parts.length}
				<fieldset class="stack">
					<legend>{t('bank.parts')}</legend>
					{#each parts as part, i (i)}
						<div class="part">
							{#if part.kind === 'account'}
								<select aria-label={t('term.ledger_account')} bind:value={part.id}>
									<option value={null}>{t('books.choose_account')}</option>
									{#each bookable as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
								</select>
							{:else}
								<span class="label">{part.label}</span>
							{/if}
							<input aria-label={t('bank.amount_eur')} inputmode="decimal" bind:value={part.amount} />
							{#if part.currency !== 'EUR'}
								<input aria-label={part.currency} placeholder={part.currency} inputmode="decimal" bind:value={part.document_amount} />
							{/if}
							<button type="button" class="ghost icon" aria-label={t('app.remove')} onclick={() => parts.splice(i, 1)}>
								<Icon name="close" size={18} />
							</button>
						</div>
					{/each}
				</fieldset>
			{/if}

			<div class="row">
				<button type="button" class="ghost" onclick={() => addAccount()}><Icon name="plus" size={18} />{t('bank.to_account')}</button>
				{#if rest !== 0}
					{#each quick as account (account.id)}
						<button type="button" class="ghost" onclick={() => addAccount(account.id)}>{account.name}</button>
					{/each}
				{/if}
				<small class:error={rest !== 0} class="muted">{t('bank.rest', { amount: eur(rest) })}</small>
			</div>
			{#if matchError}<p class="alert" role="alert">{t(matchError.fields?.parts ? 'bank.parts_must_add_up' : matchError.message)}</p>{/if}
			<div><button class="primary" disabled={rest !== 0 || !parts.length}>{t('bank.match')}</button></div>
		</form>
	{/if}
</Dialog>

<style>
	.auto {
		width: auto;
	}
	.summary {
		margin: 0;
	}
	.transactions {
		display: grid;
		gap: 0.625rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.transactions li {
		display: grid;
		gap: 0.375rem;
		padding: 0.75rem 1rem;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0.25rem 0.75rem;
	}
	.date {
		color: var(--muted);
		font-variant-numeric: tabular-nums;
	}
	.party {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.amount {
		font-variant-numeric: tabular-nums;
		font-weight: 600;
		white-space: nowrap;
	}
	.amount.in {
		color: var(--accent-text);
	}
	.purpose {
		margin: 0;
		color: var(--muted);
		font-size: 0.875rem;
		overflow-wrap: anywhere;
	}
	.parts {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.375rem;
	}
	.summary-box {
		display: grid;
		gap: 0.125rem;
		padding: 0.75rem;
		border-radius: var(--radius);
		background: var(--surface-2);
		overflow-wrap: anywhere;
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
	.candidates {
		display: grid;
		gap: 0.25rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.candidates label {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}
	.candidates input {
		width: auto;
	}
	.label {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.part {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 7rem auto;
		gap: 0.375rem;
		align-items: center;
	}
	.part:has(input + input) {
		grid-template-columns: minmax(0, 1fr) 6rem 6rem auto;
	}
	.error {
		color: var(--danger);
	}
</style>
