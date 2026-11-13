<script lang="ts">
	import { tick } from 'svelte';
	import { page } from '$app/state';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay, today } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	const owner = $derived(session.view?.user?.role === 'owner');
	const eur = (minor: number) => formatMoney(minor, 'EUR');
	const day = (iso: string) => formatDay(iso, { day: '2-digit', month: '2-digit', year: 'numeric' });
	const balance = (minor: number) =>
		minor === 0 ? eur(0) : `${eur(Math.abs(minor))} ${t(minor > 0 ? 'books.debit_short' : 'books.credit_short')}`;

	let view = $state<Schemas['BooksView']>();
	let tab = $state<'journal' | 'accounts'>('journal');
	let year = $state(Number(page.url.searchParams.get('year')) || new Date().getFullYear());
	let accounts = $state<Schemas['Account'][]>([]);
	let archivedAccounts = $state(false);
	let postings = $state<Schemas['Posting'][]>([]);
	let error = $state<ApiError>();

	let chart = $state<Schemas['Chart']>('skr03');
	let startDate = $state(`${new Date().getFullYear()}-01-01`);

	async function loadBooks() {
		const result = await call(api.GET('/api/books'));
		view = result.data;
		error = result.error;
	}

	async function loadAccounts() {
		const result = await call(api.GET('/api/accounts', { params: { query: { archived: archivedAccounts } } }));
		accounts = result.data ?? [];
	}

	async function loadJournal() {
		const result = await call(
			api.GET('/api/postings', { params: { query: { from: `${year}-01-01`, to: `${year}-12-31` } } })
		);
		postings = result.data ?? [];
		// a figure on the tax page links to the posting behind it
		if (page.url.hash) {
			await tick();
			document.querySelector(page.url.hash)?.scrollIntoView({ block: 'center' });
		}
	}

	loadBooks();
	$effect(() => {
		if (!view?.books) return;
		archivedAccounts;
		loadAccounts();
	});
	$effect(() => {
		if (!view?.books) return;
		year;
		loadJournal();
	});

	// invoices in another currency issued since the first day need the rate they are booked at
	let rates = $state<{ id: number; label: string; rate: string }[]>([]);

	async function startBooks(event: SubmitEvent) {
		event.preventDefault();
		const exchange_rates = Object.fromEntries(
			rates.filter((r) => r.rate.trim()).map((r) => [String(r.id), r.rate.trim().replace(',', '.')])
		);
		const result = await call(api.POST('/api/books', { body: { chart, start_date: startDate, exchange_rates } }));
		error = result.error;
		if (result.data) {
			view = result.data;
			toast(t('books.started'));
		}
		const needed = Object.keys(result.error?.fields ?? {})
			.filter((key) => key.startsWith('exchange_rates.'))
			.map((key) => Number(key.split('.')[1]))
			.filter((id) => !rates.some((r) => r.id === id));
		for (const id of needed) {
			const document = await call(api.GET('/api/documents/{id}', { params: { path: { id } } }));
			const d = document.data;
			rates.push({ id, label: d ? `${d.number} · ${d.customer_name} · ${d.currency}` : String(id), rate: '' });
		}
	}

	const years = $derived.by(() => {
		const first = Number(view?.books?.start_date.slice(0, 4) ?? year);
		const last = new Date().getFullYear();
		return Array.from({ length: Math.max(last - first + 1, 1) }, (_, i) => last - i);
	});

	// manual posting
	let posting = $state(false);
	let date = $state(today());
	let description = $state('');
	let lines = $state<{ account_id: number | undefined; debit: string; credit: string }[]>([]);
	let postingError = $state<ApiError>();

	function newPosting() {
		date = today();
		description = '';
		lines = [
			{ account_id: undefined, debit: '', credit: '' },
			{ account_id: undefined, debit: '', credit: '' }
		];
		postingError = undefined;
		posting = true;
	}

	const amounts = $derived(
		lines.map((l) => (parseMoney(l.debit, 'EUR') ?? 0) - (parseMoney(l.credit, 'EUR') ?? 0))
	);
	const difference = $derived(amounts.reduce((sum, a) => sum + (Number.isNaN(a) ? 0 : a), 0));

	async function savePosting(event: SubmitEvent) {
		event.preventDefault();
		if (amounts.some((a) => Number.isNaN(a)) || lines.some((l) => l.account_id == null)) {
			postingError = { ...clientError('error.invalid_input'), fields: { lines: 'invalid_format' } };
			return;
		}
		const result = await call(
			api.POST('/api/postings', {
				body: {
					date,
					description,
					lines: lines.map((l, i) => ({ account_id: l.account_id!, amount: amounts[i] }))
				}
			})
		);
		postingError = result.error;
		if (result.data) {
			posting = false;
			toast(t('books.booked'));
			await Promise.all([loadJournal(), loadAccounts()]);
		}
	}

	async function reverse(p: Schemas['Posting']) {
		if (!confirm(t('books.reverse_confirm'))) return;
		const result = await call(
			api.POST('/api/postings/{id}/reverse', { params: { path: { id: p.id } }, body: {} })
		);
		error = result.error;
		if (result.data) {
			toast(t('books.reversed'));
			await Promise.all([loadJournal(), loadAccounts()]);
		}
	}

	// accounts
	let accountDialog = $state(false);
	let editingAccount = $state<Schemas['Account']>();
	let number = $state('');
	let name = $state('');
	let kind = $state<Schemas['AccountType']>('expense');
	let euerLine = $state<Schemas['EuerLine'] | null>(null);
	let accountError = $state<ApiError>();
	const euerLines: Schemas['EuerLine'][] = [
		'12', '15', '16', '17', '18', '27', '29', '30', '32', '33', '36', '38', '39', '44', '55', '56', '57', '58', '60', '63'
	];
	const lineDefault = (type: Schemas['AccountType']): Schemas['EuerLine'] | null =>
		type === 'income' ? '15' : type === 'expense' ? '60' : null;

	function editAccount(account?: Schemas['Account']) {
		editingAccount = account;
		number = account?.number ?? '';
		name = account?.name ?? '';
		kind = account?.type ?? 'expense';
		euerLine = account ? (account.euer_line ?? null) : lineDefault(kind);
		accountError = undefined;
		accountDialog = true;
	}

	async function saveAccount(event: SubmitEvent) {
		event.preventDefault();
		const result = editingAccount
			? await call(
					api.PUT('/api/accounts/{id}', {
						params: { path: { id: editingAccount.id } },
						body: { name, archived: editingAccount.archived, euer_line: euerLine }
					})
				)
			: await call(api.POST('/api/accounts', { body: { number, name, type: kind, euer_line: euerLine } }));
		accountError = result.error;
		if (result.data) {
			accountDialog = false;
			toast(t('app.saved'));
			await loadAccounts();
		}
	}

	async function setArchived(account: Schemas['Account'], archived: boolean) {
		const result = await call(
			api.PUT('/api/accounts/{id}', {
				params: { path: { id: account.id } },
				body: { name: account.name, archived, euer_line: account.euer_line }
			})
		);
		error = result.error;
		await loadAccounts();
	}

	const open = $derived(accounts.filter((a) => !a.archived));
</script>

<div class="page-head">
	<div>
		<h1>{t('books.title')}</h1>
		<p class="muted">
			{#if view?.books}
				{t('books.intro', {
					chart: view.books.chart.toUpperCase(),
					start: day(view.books.start_date),
					taxation: t(`business.taxation.${view.taxation}`)
				})}
			{:else}
				{t('books.intro_not_started')}
			{/if}
		</p>
	</div>
	{#if owner && view?.books}
		{#if tab === 'journal'}
			<button class="primary" onclick={newPosting}><Icon name="plus" size={18} />{t('books.new_posting')}</button>
		{:else}
			<button class="primary" onclick={() => editAccount()}><Icon name="plus" size={18} />{t('books.new_account')}</button>
		{/if}
	{/if}
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if view && !view.books}
	{#if owner}
		<form class="card stack start" onsubmit={startBooks}>
			<h2>{t('books.start')}</h2>
			<p class="muted">{t('books.start_hint')}</p>
			<Field label={t('term.chart_of_accounts')}>
				<select bind:value={chart}>
					<option value="skr03">SKR 03</option>
					<option value="skr04">SKR 04</option>
				</select>
			</Field>
			<Field label={t('books.start_date')} error={error?.fields?.start_date}>
				<input type="date" bind:value={startDate} />
			</Field>
			{#if rates.length}
				<p class="muted">{t('books.rates_hint')}</p>
				{#each rates as entry (entry.id)}
					<Field label={entry.label} error={error?.fields?.[`exchange_rates.${entry.id}`]} note={t('books.rate_note')}>
						<input inputmode="decimal" bind:value={entry.rate} />
					</Field>
				{/each}
			{/if}
			<div><button class="primary">{t('books.start')}</button></div>
		</form>
	{:else}
		<p class="card empty">{t('books.not_started')}</p>
	{/if}
{:else if view?.books}
	<div class="stack">
		<div class="row">
			<Segmented
				label={t('books.title')}
				options={[
					{ value: 'journal', label: t('term.journal') },
					{ value: 'accounts', label: t('books.accounts') }
				]}
				value={tab}
				onchange={(v) => (tab = v as typeof tab)}
			/>
			{#if tab === 'journal'}
				<select class="year" aria-label={t('books.year')} bind:value={year}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{:else}
				<Segmented
					label={t('books.accounts')}
					options={[
						{ value: 'active', label: t('app.active') },
						{ value: 'archived', label: t('app.archived') }
					]}
					value={archivedAccounts ? 'archived' : 'active'}
					onchange={(v) => (archivedAccounts = v === 'archived')}
				/>
			{/if}
		</div>

		{#if tab === 'journal'}
			{#if postings.length}
				<ul class="journal">
					{#each postings as p (p.id)}
						<li class="card" class:reversed={p.reversed_by != null} id="posting-{p.id}">
							<div class="head">
								<span class="date">{day(p.date)}</span>
								{#if p.document_id}
									<a href="/documents/{p.document_id}"><strong>{p.description}</strong></a>
								{:else if p.asset_id}
									<a href="/assets"><strong>{p.description}</strong></a>
								{:else}
									<strong>{p.description}</strong>
								{/if}
								{#if p.reversed_by != null}<span class="badge">{t('books.is_reversed')}</span>{/if}
								{#if p.reverses_id != null}<span class="badge">{t('term.reversal')}</span>{/if}
								{#if owner && p.reversed_by == null && p.reverses_id == null && p.document_id == null && p.payment_id == null && p.asset_id == null}
									<button class="ghost push" onclick={() => reverse(p)}>{t('books.reverse')}</button>
								{/if}
							</div>
							<table>
								<tbody>
									{#each p.lines as line (line.account_id)}
										<tr>
											<td class="account"><span class="number">{line.number}</span> {line.name}</td>
											<td class="amount">{line.amount > 0 ? eur(line.amount) : ''}</td>
											<td class="amount">{line.amount < 0 ? eur(-line.amount) : ''}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="card empty">{t('books.no_postings')}</p>
			{/if}
		{:else if accounts.length}
			<div class="table-wrap stacked">
				<table>
					<thead>
						<tr>
							<th>{t('books.number')}</th>
							<th>{t('books.account_name')}</th>
							<th>{t('books.type')}</th>
							<th>{t('books.euer_line')}</th>
							<th class="amount">{t('books.balance')}</th>
							{#if owner}<th></th>{/if}
						</tr>
					</thead>
					<tbody>
						{#each accounts as account (account.id)}
							<tr>
								<td data-label={t('books.number')}><span class="number">{account.number}</span></td>
								<td data-label={t('books.account_name')}>{account.name}</td>
								<td data-label={t('books.type')}>{t(`books.type.${account.type}`)}</td>
								<td data-label={t('books.euer_line')}>{account.euer_line ?? '–'}</td>
								<td data-label={t('books.balance')} class="amount">{balance(account.balance)}</td>
								{#if owner}
									<td class="actions">
										<button class="ghost" onclick={() => editAccount(account)}>{t('app.edit')}</button>
										{#if !account.role}
											<button class="ghost" onclick={() => setArchived(account, !account.archived)}>
												{t(account.archived ? 'customers.restore' : 'customers.archive')}
											</button>
										{/if}
									</td>
								{/if}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{:else}
			<p class="card empty">{t('books.no_accounts')}</p>
		{/if}
	</div>
{/if}

<Dialog bind:open={posting} title={t('books.new_posting')}>
	<form class="stack" onsubmit={savePosting}>
		<div class="pair">
			<Field label={t('books.date')} error={postingError?.fields?.date}>
				<input type="date" bind:value={date} />
			</Field>
			<Field label={t('books.description')} error={postingError?.fields?.description}>
				<input bind:value={description} placeholder={t('books.description_hint')} />
			</Field>
		</div>
		<fieldset class="stack">
			<legend>{t('books.lines')}</legend>
			{#each lines as line, i (i)}
				<div class="line">
					<select aria-label={t('term.ledger_account')} bind:value={line.account_id}>
						<option value={undefined}>{t('books.choose_account')}</option>
						{#each open as account (account.id)}
							<option value={account.id}>{account.number} {account.name}</option>
						{/each}
					</select>
					<input aria-label={t('books.debit')} placeholder={t('books.debit')} inputmode="decimal" bind:value={line.debit} />
					<input aria-label={t('books.credit')} placeholder={t('books.credit')} inputmode="decimal" bind:value={line.credit} />
					<button type="button" class="ghost icon" aria-label={t('app.remove')} disabled={lines.length <= 2} onclick={() => lines.splice(i, 1)}>
						<Icon name="close" size={18} />
					</button>
				</div>
			{/each}
			<div class="row">
				<button type="button" class="ghost" onclick={() => lines.push({ account_id: undefined, debit: '', credit: '' })}>
					<Icon name="plus" size={18} />{t('books.add_line')}
				</button>
				<small class:error={difference !== 0} class="muted">{t('books.difference', { amount: eur(difference) })}</small>
			</div>
			{#if postingError?.fields?.lines}<small class="error">{t('books.unbalanced')}</small>{/if}
		</fieldset>
		{#if postingError}<p class="alert" role="alert">{t(postingError.message)}</p>{/if}
		<div><button class="primary" disabled={difference !== 0}>{t('books.book')}</button></div>
	</form>
</Dialog>

<Dialog bind:open={accountDialog} title={t(editingAccount ? 'books.edit_account' : 'books.new_account')}>
	<form class="stack" onsubmit={saveAccount}>
		<Field label={t('books.number')} error={accountError?.fields?.number}>
			<input inputmode="numeric" bind:value={number} readonly={!!editingAccount} />
		</Field>
		<Field label={t('books.account_name')} error={accountError?.fields?.name}>
			<input bind:value={name} />
		</Field>
		<Field label={t('books.type')}>
			<select bind:value={kind} disabled={!!editingAccount} onchange={() => (euerLine = lineDefault(kind))}>
				{#each ['asset', 'liability', 'equity', 'income', 'expense'] as const as type (type)}
					<option value={type}>{t(`books.type.${type}`)}</option>
				{/each}
			</select>
		</Field>
		<Field label={t('books.euer_line')} error={accountError?.fields?.euer_line}>
			<select bind:value={euerLine}>
				<option value={null}>{t('books.euer_none')}</option>
				{#each euerLines as line (line)}
					<option value={line}>{t('tax.zeile', { line })} · {t(`tax.euer.${line}`)}</option>
				{/each}
			</select>
		</Field>
		{#if accountError}<p class="alert" role="alert">{t(accountError.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<style>
	.start {
		max-width: 32rem;
	}
	.year {
		width: auto;
	}
	.journal {
		display: grid;
		gap: 0.75rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.journal li {
		padding: 0.875rem 1rem;
	}
	.journal li.reversed {
		opacity: 0.6;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.375rem 0.75rem;
		margin-bottom: 0.5rem;
	}
	.head .push {
		margin-left: auto;
	}
	.date,
	.number,
	.amount {
		font-variant-numeric: tabular-nums;
	}
	.date,
	.number {
		color: var(--muted);
	}
	.journal table {
		width: 100%;
		border-collapse: collapse;
	}
	.journal td {
		padding: 0.25rem 0;
	}
	.account {
		overflow-wrap: anywhere;
	}
	.amount {
		width: 8rem;
		text-align: right;
		white-space: nowrap;
	}
	.actions {
		text-align: right;
		white-space: nowrap;
	}
	.pair {
		display: grid;
		grid-template-columns: 10rem 1fr;
		gap: 0.75rem;
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
	.line {
		display: grid;
		grid-template-columns: 1fr 6.5rem 6.5rem auto;
		gap: 0.5rem;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
	@media (max-width: 40rem) {
		.pair {
			grid-template-columns: 1fr;
		}
		.line {
			grid-template-columns: 1fr 1fr auto;
		}
		.line select {
			grid-column: 1 / -1;
		}
		.amount {
			width: 6rem;
		}
	}
</style>
