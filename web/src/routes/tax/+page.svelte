<script lang="ts">
	import { page } from '$app/state';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay, isoDay } from '#lib/dates.ts';
	import { daysUntil, deadlineTitle, monthName } from '#lib/deadlines.ts';
	import { confirm } from '#lib/confirm.svelte.ts';
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

	/** The last finished period of the business's VAT period, quarterly when unset. */
	function lastPeriod(vatPeriod: Schemas['VatPeriod'] | null | undefined): { year: number; period: string } {
		const month = now.getMonth();
		if (vatPeriod === 'annual') return { year: now.getFullYear() - 1, period: 'y' };
		if (vatPeriod === 'monthly') {
			const last = new Date(now.getFullYear(), month - 1, 1);
			return { year: last.getFullYear(), period: `m${last.getMonth() + 1}` };
		}
		const quarter = Math.floor(month / 3);
		return quarter === 0 ? { year: now.getFullYear() - 1, period: 'q4' } : { year: now.getFullYear(), period: `q${quarter}` };
	}

	let vatYear = $state(lastPeriod(null).year);
	let period = $state(lastPeriod(null).period);
	let periodKnown = $state(false);
	call(api.GET('/api/business')).then((result) => {
		({ year: vatYear, period } = lastPeriod(result.data?.values.vat_period));
		periodKnown = true;
	});
	let euerYear = $state(now.getFullYear() - (now.getMonth() < 6 ? 1 : 0));

	const years = $derived.by(() => {
		const first = Number(books?.books?.start_date.slice(0, 4) ?? now.getFullYear());
		return Array.from({ length: Math.max(now.getFullYear() - first + 1, 1) }, (_, i) => now.getFullYear() - i);
	});
	// in the books' first year there is no finished year before them yet
	$effect(() => {
		if (books?.books && periodKnown && !years.includes(vatYear)) vatYear = years.at(-1)!;
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
		const asked = range(vatYear, period);
		const result = await call(api.GET('/api/tax/vat-return', { params: { query: asked } }));
		// a slower answer for a period chosen before must not overwrite the current one
		if (asked.from !== range(vatYear, period).from || asked.to !== range(vatYear, period).to) return;
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
		if (!books?.books || tab !== 'vat' || !periodKnown) return;
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
		if (!(await confirm({ text: t('tax.close_confirm', { year }), action: t('tax.close_year', { year }), danger: true }))) return;
		const result = await call(api.POST('/api/years/{year}/close', { params: { path: { year } } }));
		error = result.error;
		if (result.data) {
			toast(t('tax.year_closed', { year }));
			archives = (await call(api.GET('/api/years'))).data;
		}
	}

	async function renewFeed() {
		if (feed?.created_at && !(await confirm({ text: t('tax.calendar_renew_confirm'), action: t('tax.calendar_renew'), danger: true }))) return;
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

	const income = $derived((euer?.lines ?? []).filter((l) => parseInt(l.key) <= 18));
	const expenses = $derived((euer?.lines ?? []).filter((l) => parseInt(l.key) > 18));
	const euerRow = (l: Schemas['Figure']) => ({
		...l,
		label: t(`tax.euer.${l.key}`),
		code: t(l.kennzahl ? 'tax.zeile_kz' : 'tax.zeile', { line: parseInt(l.key), kz: l.kennzahl ?? '' })
	});
	let euerView = $state<'overview' | 'elster'>('overview');
	/** As typed into Mein ELSTER, whatever the UI language. */
	const elsterAmount = (minor: number) =>
		new Intl.NumberFormat('de-DE', { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(minor / 100);
	const entriesOf = (key: string) => (euer?.entries ?? []).filter((e) => e.key === key);
	const homeOfficeOver =$derived((euer?.lines.find((l) => l.key === '66')?.amount ?? 0) > 126000);
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

{#snippet elsterRow(label: string, minor: number)}
	<div class="elster-row">
		<span>{label}</span>
		<span class="amount">{elsterAmount(minor)}</span>
		<button type="button" class="ghost icon" aria-label={t('app.copy')} onclick={() => copy(elsterAmount(minor))}>
			<Icon name="copy" size={16} />
		</button>
	</div>
{/snippet}

{#snippet elsterLine(line: Schemas['Figure'])}
	{@const row = euerRow(line)}
	<section class="card elster">
		<h3><span class="code">{row.code}</span> {row.label}</h3>
		{#each entriesOf(line.key) as entry (entry.account_id)}
			{@render elsterRow(entry.name, entry.amount)}
		{/each}
		<div class="elster-row sum">
			<span>{t('tax.elster_line_sum')}</span>
			<span class="amount">{elsterAmount(line.amount)}</span>
			<span></span>
		</div>
	</section>
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
									{#if closed.archive}
										<code title={t('tax.checksum')}>SHA-256 {closed.archive}</code>
										{#if closed.pruned}<small class="muted">{t('retention.pruned')}</small>{/if}
									{:else}
										<small class="muted">{t('retention.anonymised')}</small>
									{/if}
								</div>
								{#if closed.archive}
									<a class="button ghost" href="/api/years/{closed.year}/archive" download>
										<Icon name="archive" size={18} />{t('tax.download')}
									</a>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}
				{#if owner && archives?.next != null}
					{#if archives.review_answers.length}
						<p class="muted">
							{t('tax.review_answers', { count: archives.review_answers.length })}
							<a href="/process-documentation">{t('tax.review')}</a>
						</p>
					{/if}
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
			<Segmented
				label={t('tax.euer_view')}
				options={[
					{ value: 'overview', label: t('tax.euer_overview') },
					{ value: 'elster', label: t('tax.euer_elster') }
				]}
				value={euerView}
				onchange={(v) => (euerView = v as typeof euerView)}
			/>
		{/if}
		{#if tab === 'euer' && euer && euerView === 'elster'}
			<p class="muted">{t('tax.elster_intro')}</p>
			{#if euer.small_business}<p class="muted">{t('tax.elster_small_business')}</p>{/if}
			{#if homeOfficeOver}<p class="alert" role="status">{t('tax.home_office_over')}</p>{/if}
			<h2>{t('tax.elster_income')}</h2>
			{#each income as line (line.key)}{@render elsterLine(line)}{:else}<p class="card empty">{t('tax.nothing')}</p>{/each}
			<h2>{t('tax.elster_expenses')}</h2>
			{#each expenses as line (line.key)}{@render elsterLine(line)}{:else}<p class="card empty">{t('tax.nothing')}</p>{/each}
			<h2>{t('tax.elster_profit')}</h2>
			<div class="card elster">
				{@render elsterRow(t('tax.elster_sum_income'), euer.income)}
				{@render elsterRow(t('tax.elster_sum_expenses'), euer.expenses)}
				{@render elsterRow(t(euer.profit < 0 ? 'tax.loss' : 'tax.profit'), euer.profit)}
			</div>
			{#if euer.withdrawals || euer.deposits}
				<h2>{t('tax.elster_private')}</h2>
				<div class="card elster">
					{#if euer.withdrawals}{@render elsterRow(t('tax.elster_withdrawals'), euer.withdrawals)}{/if}
					{#if euer.deposits}{@render elsterRow(t('tax.elster_deposits'), euer.deposits)}{/if}
				</div>
			{/if}
		{:else if tab === 'euer' && euer}
			{#if euer.small_business}<p class="muted">{t('tax.euer_small_business')}</p>{/if}
			<h2>{t('tax.income')}</h2>
			{#if income.length}
				{@render table(income.map(euerRow), false)}
			{:else}
				<p class="card empty">{t('tax.nothing')}</p>
			{/if}
			<h2>{t('tax.expenses')}</h2>
			{#if expenses.length}
				{#if homeOfficeOver}<p class="alert" role="status">{t('tax.home_office_over')}</p>{/if}
				{@render table(expenses.map(euerRow), false)}
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
	.elster {
		display: grid;
		gap: 0.25rem;
		padding: 0.75rem 1rem;
	}
	.elster h3 {
		margin: 0 0 0.25rem;
		font-size: 0.9375rem;
	}
	.elster-row {
		display: grid;
		grid-template-columns: 1fr auto 2rem;
		align-items: center;
		gap: 0.75rem;
		min-height: 2rem;
	}
	.elster-row span:first-child {
		overflow-wrap: anywhere;
	}
	.elster-row.sum {
		border-top: 1px solid var(--border);
		padding-top: 0.25rem;
		color: var(--muted);
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
