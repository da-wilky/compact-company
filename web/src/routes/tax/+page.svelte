<script lang="ts">
	import { page } from '$app/state';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay, isoDay } from '#lib/dates.ts';
	import { daysUntil, deadlineTitle, monthName } from '#lib/deadlines.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	const eur = (minor: number) => formatMoney(minor, 'EUR');
	const day = (iso: string) => formatDay(iso, { day: '2-digit', month: '2-digit', year: 'numeric' });

	type Tab = 'vat' | 'euer' | 'exports' | 'deadlines';
	const tabs: Tab[] = ['vat', 'euer', 'exports', 'deadlines'];
	const asked = page.url.searchParams.get('tab') as Tab;
	let tab = $state<Tab>(tabs.includes(asked) ? asked : 'vat');
	let error = $state<ApiError>();
	let books = $state<Schemas['BooksView']>();

	const now = new Date();
	const lastMonth = new Date(now.getFullYear(), now.getMonth() - 1, 1);
	let vatYear = $state(lastMonth.getFullYear());
	let period = $state(`m${lastMonth.getMonth() + 1}`);
	let euerYear = $state(now.getFullYear() - (now.getMonth() < 6 ? 1 : 0));

	const years = $derived.by(() => {
		const first = Number(books?.books?.start_date.slice(0, 4) ?? now.getFullYear());
		return Array.from({ length: Math.max(now.getFullYear() - first + 1, 1) }, (_, i) => now.getFullYear() - i);
	});

	const periods = $derived([
		...Array.from({ length: 12 }, (_, i) => ({ value: `m${i + 1}`, label: monthName(i + 1) })),
		...[1, 2, 3, 4].map((q) => ({ value: `q${q}`, label: t('tax.quarter', { quarter: q }) })),
		{ value: 'y', label: t('tax.whole_year') }
	]);

	function range(year: number, period: string): { from: string; to: string } {
		const pad = (n: number) => String(n).padStart(2, '0');
		const last = (month: number) => new Date(year, month, 0).getDate();
		const [first, final] =
			period === 'y'
				? [1, 12]
				: period.startsWith('q')
					? [Number(period.slice(1)) * 3 - 2, Number(period.slice(1)) * 3]
					: [Number(period.slice(1)), Number(period.slice(1))];
		return { from: `${year}-${pad(first)}-01`, to: `${year}-${pad(final)}-${pad(last(final))}` };
	}

	let vat = $state<Schemas['VatReturn']>();
	let euer = $state<Schemas['EuerView']>();

	async function loadBooks() {
		const result = await call(api.GET('/api/books'));
		books = result.data;
		error = result.error;
	}

	async function loadVat() {
		const result = await call(api.GET('/api/tax/vat-return', { params: { query: range(vatYear, period) } }));
		vat = result.data;
		error = result.error;
	}

	async function loadEuer() {
		const result = await call(api.GET('/api/tax/euer', { params: { query: { year: euerYear } } }));
		euer = result.data;
		error = result.error;
	}

	loadBooks();
	$effect(() => {
		if (!books?.books || tab !== 'vat') return;
		vatYear;
		period;
		loadVat();
	});
	$effect(() => {
		if (!books?.books || tab !== 'euer') return;
		euerYear;
		loadEuer();
	});

	const vatRows = $derived(
		(vat?.figures ?? []).map((f) => ({
			...f,
			label: t(`tax.vat.${f.key}`),
			code: f.key.endsWith('_tax') ? '' : `Kz ${f.key}`
		}))
	);
	const owner = $derived(session.view?.user?.role === 'owner');
	let deadlines = $state<Schemas['Deadline'][]>([]);
	let feed = $state<Schemas['Feed']>();

	$effect(() => {
		if (tab !== 'deadlines') return;
		call(api.GET('/api/deadlines')).then((r) => (deadlines = r.data ?? []));
		if (owner) call(api.GET('/api/calendar-feed')).then((r) => (feed = r.data));
	});

	let archives = $state<Schemas['Years']>();
	$effect(() => {
		if (tab === 'exports' && books?.books) call(api.GET('/api/years')).then((r) => (archives = r.data));
	});

	async function closeYear(year: number) {
		if (!confirm(t('tax.close_confirm', { year }))) return;
		const result = await call(api.POST('/api/years/{year}/close', { params: { path: { year } } }));
		error = result.error;
		if (result.data) {
			toast(t('tax.year_closed', { year }));
			archives = (await call(api.GET('/api/years'))).data;
		}
	}

	async function renewFeed() {
		if (feed?.created_at && !confirm(t('tax.calendar_renew_confirm'))) return;
		const result = await call(api.POST('/api/calendar-feed'));
		error = result.error;
		if (result.data) feed = result.data;
	}

	async function deleteFeed() {
		const result = await call(api.DELETE('/api/calendar-feed'));
		error = result.error;
		if (result.data) feed = result.data;
	}

	async function copy(link: string) {
		await navigator.clipboard.writeText(link);
		toast(t('app.copied'));
	}

	const income = $derived((euer?.lines ?? []).filter((l) => Number(l.key) <= 18));
	const expenses = $derived((euer?.lines ?? []).filter((l) => Number(l.key) > 18));
</script>

{#snippet sources(figure: Schemas['Figure'])}
	{#if figure.sources.length}
		<details>
			<summary>{t('tax.postings', { count: figure.sources.length })}</summary>
			<ul class="sources">
				{#each figure.sources as source (source.posting_id)}
					<li>
						<span class="date">{day(source.date)}</span>
						<a href="/books?year={source.date.slice(0, 4)}#posting-{source.posting_id}">{source.description}</a>
						<span class="amount">{eur(source.amount)}</span>
					</li>
				{/each}
			</ul>
		</details>
	{/if}
{/snippet}

{#snippet table(rows: (Schemas['Figure'] & { label: string; code: string })[], entered: boolean)}
	<div class="table-wrap stacked">
		<table>
			<thead>
				<tr>
					<th>{t('tax.line')}</th>
					<th>{t('tax.description')}</th>
					<th class="amount">{t('tax.amount')}</th>
					{#if entered}<th class="amount">{t('tax.entered')}</th>{/if}
				</tr>
			</thead>
			<tbody>
				{#each rows as row (row.key)}
					<tr>
						<td data-label={t('tax.line')} class="code">{row.code}</td>
						<td data-label={t('tax.description')}>
							{row.label}
							{@render sources(row)}
						</td>
						<td data-label={t('tax.amount')} class="amount">{eur(row.amount)}</td>
						{#if entered}<td data-label={t('tax.entered')} class="amount"><strong>{row.entered}</strong></td>{/if}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/snippet}

<div class="page-head">
	<div>
		<h1>{t('tax.title')}</h1>
		<p class="muted">{t('tax.intro')}</p>
	</div>
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if books}
	<div class="stack">
		<div class="row">
			<Segmented
				label={t('tax.title')}
				options={[
					{ value: 'vat', label: t('tax.vat_return') },
					{ value: 'euer', label: t('tax.euer') },
					{ value: 'exports', label: t('tax.exports') },
					{ value: 'deadlines', label: t('tax.deadlines') }
				]}
				value={tab}
				onchange={(v) => (tab = v as typeof tab)}
			/>
			{#if books.books && (tab === 'vat' || tab === 'exports')}
				<select class="narrow" aria-label={t('tax.period')} bind:value={period}>
					{#each periods as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
				</select>
				<select class="narrow" aria-label={t('books.year')} bind:value={vatYear}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{:else if books.books && tab === 'euer'}
				<select class="narrow" aria-label={t('books.year')} bind:value={euerYear}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{/if}
		</div>

		{#if tab === 'deadlines'}
			{#if deadlines.length}
				<ul class="deadlines">
					{#each deadlines as deadline (deadline.key)}
						{@const left = daysUntil(deadline.due)}
						<li class="card" class:past={left < 0}>
							<div>
								<strong>{deadlineTitle(deadline)}</strong>
								<small class="muted">{day(deadline.from)} – {day(deadline.to)}</small>
							</div>
							<div class="due">
								<strong>{day(deadline.due)}</strong>
								<small class:soon={left >= 0 && left <= 7}>
									{left < 0 ? t('tax.due_past') : left === 0 ? t('tax.due_today') : t('tax.due_in', { days: left })}
								</small>
							</div>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="card empty">{t('tax.no_deadlines')}</p>
			{/if}
			<p class="muted small">{t('tax.deadlines_hint')}</p>
			{#if owner && feed}
				<section class="card stack">
					<h2>{t('tax.calendar')}</h2>
					<p class="muted">{t(feed.created_at ? 'tax.calendar_active' : 'tax.calendar_hint')}</p>
					{#if feed.link}
						<div class="link">
							<input readonly value={feed.link} aria-label={t('tax.calendar')} />
							<button type="button" class="ghost icon" aria-label={t('app.copy')} onclick={() => copy(feed!.link!)}>
								<Icon name="copy" size={18} />
							</button>
						</div>
						<small class="muted">{t('tax.calendar_once')}</small>
					{/if}
					<div class="row">
						<button class="primary" onclick={renewFeed}><Icon name="calendar" size={18} />{t(feed.created_at ? 'tax.calendar_renew' : 'tax.calendar_create')}</button>
						{#if feed.created_at}<button class="ghost" onclick={deleteFeed}>{t('tax.calendar_delete')}</button>{/if}
					</div>
				</section>
			{/if}
		{:else if !books.books}
			<p class="card empty">{t('tax.not_started')}</p>
		{:else if tab === 'exports'}
			{@const { from, to } = range(vatYear, period)}
			<section class="card stack export">
				<h2>{t('tax.datev')}</h2>
				<p class="muted">{t('tax.datev_hint', { from: day(from), to: day(to) })}</p>
				<div>
					<a class="button primary" href="/api/exports/datev?from={from}&to={to}" download>
						<Icon name="pdf" size={18} />{t('tax.download')}
					</a>
				</div>
			</section>
			<section class="card stack">
				<h2>{t('tax.archive')}</h2>
				<p class="muted">{t('tax.archive_hint')}</p>
				{#if archives?.closed.length}
					<ul class="archives">
						{#each archives.closed as closed (closed.year)}
							<li>
								<div>
									<strong>{closed.year}</strong>
									<small class="muted">{t('tax.closed_by', { name: closed.closed_by, date: day(isoDay(new Date(closed.closed_at * 1000))) })}</small>
									<code title={t('tax.checksum')}>SHA-256 {closed.archive}</code>
								</div>
								<a class="button ghost" href="/api/years/{closed.year}/archive" download>
									<Icon name="archive" size={18} />{t('tax.download')}
								</a>
							</li>
						{/each}
					</ul>
				{/if}
				{#if owner && archives?.next != null}
					<div>
						<button class="primary" onclick={() => closeYear(archives!.next!)}>
							<Icon name="archive" size={18} />{t('tax.close_year', { year: archives.next })}
						</button>
					</div>
				{/if}
			</section>
		{:else if tab === 'vat' && vat}
			{#if vat.basis}
				<p class="muted">{t(`tax.basis.${vat.basis}`)}</p>
				{@render table(vatRows, true)}
			{:else}
				<p class="card empty">{t('tax.small_business_no_return')}</p>
			{/if}
		{:else if tab === 'euer' && euer}
			{#if euer.small_business}<p class="muted">{t('tax.euer_small_business')}</p>{/if}
			<h2>{t('tax.income')}</h2>
			{#if income.length}
				{@render table(income.map((l) => ({ ...l, label: t(`tax.euer.${l.key}`), code: t('tax.zeile', { line: l.key }) })), false)}
			{:else}
				<p class="card empty">{t('tax.nothing')}</p>
			{/if}
			<h2>{t('tax.expenses')}</h2>
			{#if expenses.length}
				{@render table(expenses.map((l) => ({ ...l, label: t(`tax.euer.${l.key}`), code: t('tax.zeile', { line: l.key }) })), false)}
			{:else}
				<p class="card empty">{t('tax.nothing')}</p>
			{/if}
			<dl class="card totals">
				<dt>{t('tax.income')}</dt>
				<dd>{eur(euer.income)}</dd>
				<dt>{t('tax.expenses')}</dt>
				<dd>{eur(euer.expenses)}</dd>
				<dt><strong>{t(euer.profit < 0 ? 'tax.loss' : 'tax.profit')}</strong></dt>
				<dd><strong>{eur(euer.profit)}</strong></dd>
			</dl>
		{/if}
		{#if (tab === 'vat' && vat?.basis) || (tab === 'euer' && euer)}
			<p class="muted small">{t('tax.check_forms')}</p>
		{/if}
	</div>
{/if}

<style>
	.narrow {
		width: auto;
	}
	h2 {
		margin: 0.5rem 0 0;
	}
	.code,
	.date,
	.amount {
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.code,
	.date {
		color: var(--muted);
	}
	td.amount,
	th.amount {
		text-align: right;
	}
	details {
		margin-top: 0.25rem;
		font-size: 0.875rem;
	}
	summary {
		color: var(--muted);
		cursor: pointer;
	}
	.sources {
		display: grid;
		gap: 0.25rem;
		margin: 0.375rem 0 0;
		padding: 0;
		list-style: none;
	}
	.sources li {
		display: grid;
		grid-template-columns: auto 1fr auto;
		gap: 0.75rem;
	}
	.sources a {
		overflow-wrap: anywhere;
	}
	.totals {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 0.5rem 1rem;
		max-width: 28rem;
		margin: 0;
		padding: 1rem;
	}
	.totals dd {
		margin: 0;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.small {
		font-size: 0.8125rem;
	}
	.deadlines {
		display: grid;
		gap: 0.5rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.deadlines li {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
		padding: 0.75rem 1rem;
	}
	.deadlines li > div {
		display: flex;
		flex-direction: column;
		gap: 0.125rem;
		min-width: 0;
	}
	.deadlines .past {
		opacity: 0.6;
	}
	.due {
		align-items: flex-end;
		text-align: right;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.soon {
		color: var(--danger);
		font-weight: 600;
	}
	.link {
		display: flex;
		gap: 0.5rem;
	}
	.archives {
		display: grid;
		gap: 0.75rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.archives li {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
	}
	.archives li > div {
		display: flex;
		flex-direction: column;
		gap: 0.125rem;
		min-width: 0;
	}
	.archives code {
		color: var(--muted);
		font-size: 0.75rem;
		overflow-wrap: anywhere;
	}
	.link input {
		min-width: 0;
		flex: 1;
	}
</style>
