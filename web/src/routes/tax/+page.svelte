<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay } from '#lib/dates.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';

	const eur = (minor: number) => formatMoney(minor, 'EUR');
	const day = (iso: string) => formatDay(iso, { day: '2-digit', month: '2-digit', year: 'numeric' });

	let tab = $state<'vat' | 'euer' | 'exports'>('vat');
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

	const monthName = (m: number) => new Date(2000, m - 1, 1).toLocaleDateString(i18n.locale, { month: 'long' });
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

{#if books && !books.books}
	<p class="card empty">{t('tax.not_started')}</p>
{:else if books}
	<div class="stack">
		<div class="row">
			<Segmented
				label={t('tax.title')}
				options={[
					{ value: 'vat', label: t('tax.vat_return') },
					{ value: 'euer', label: t('tax.euer') },
					{ value: 'exports', label: t('tax.exports') }
				]}
				value={tab}
				onchange={(v) => (tab = v as typeof tab)}
			/>
			{#if tab === 'vat' || tab === 'exports'}
				<select class="narrow" aria-label={t('tax.period')} bind:value={period}>
					{#each periods as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
				</select>
				<select class="narrow" aria-label={t('books.year')} bind:value={vatYear}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{:else}
				<select class="narrow" aria-label={t('books.year')} bind:value={euerYear}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{/if}
		</div>

		{#if tab === 'exports'}
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
</style>
