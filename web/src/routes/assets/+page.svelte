<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
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
	const kinds: Schemas['AssetKind'][] = ['intangible', 'vehicle', 'office', 'other'];
	const thisYear = new Date().getFullYear();

	let view = $state<Schemas['Assets']>();
	let accounts = $state<Schemas['Account'][]>([]);
	let error = $state<ApiError>();
	let tab = $state<'assets' | 'register'>('assets');
	let year = $state(thisYear - 1);
	let register = $state<Schemas['Register']>();

	async function load() {
		const result = await call(api.GET('/api/assets'));
		view = result.data;
		error = result.error;
	}

	async function loadAccounts() {
		const result = await call(api.GET('/api/accounts', { params: { query: { archived: false } } }));
		accounts = (result.data ?? []).filter((a) => a.type === 'asset' && !a.role && a.number.startsWith('0'));
	}

	async function loadRegister() {
		const result = await call(api.GET('/api/assets/register', { params: { query: { year } } }));
		register = result.data;
	}

	load();
	loadAccounts();
	$effect(() => {
		if (tab !== 'register') return;
		year;
		loadRegister();
	});

	const accountName = (id: number) => {
		const a = accounts.find((a) => a.id === id);
		return a ? `${a.number} ${a.name}` : '';
	};
	const years = Array.from({ length: 8 }, (_, i) => thisYear - i);

	/** The book value now: at the end of the last year in its schedule that has begun. */
	function bookValue(asset: Schemas['AssetView']) {
		if (asset.disposed) return 0;
		const past = asset.schedule.filter((s) => s.year <= thisYear);
		return past.length ? past[past.length - 1].book_value : asset.cost;
	}

	async function bookDepreciation(through: number) {
		const result = await call(api.POST('/api/assets/depreciation', { body: { year: through } }));
		error = result.error;
		if (result.data) {
			view = result.data;
			toast(t('assets.depreciation_booked', { year: through }));
		}
	}

	// new or changed asset
	let dialog = $state(false);
	let editing = $state<Schemas['AssetView']>();
	let source = $state<'receipt' | 'hand'>('receipt');
	let lineId = $state<number>();
	let accountId = $state<number>();
	let name = $state('');
	let kind = $state<Schemas['AssetKind']>('office');
	let method = $state<Schemas['Method']>('straight_line');
	let life = $state('3');
	let acquired = $state('');
	let cost = $state('');
	let formError = $state<ApiError>();

	const candidates = $derived([
		...(editing?.receipt_line_id != null && editing.receipt_id != null
			? [
					{
						receipt_line_id: editing.receipt_line_id,
						receipt_id: editing.receipt_id,
						description: editing.name,
						supplier_name: '',
						date: editing.acquired,
						account_id: editing.account_id,
						cost: editing.cost,
						net: editing.net
					}
				]
			: []),
		...(view?.candidates ?? [])
	]);
	const candidate = $derived(candidates.find((c) => c.receipt_line_id === lineId));
	const net = $derived(source === 'receipt' ? candidate?.net : parseMoney(cost, 'EUR'));
	const overLimit = $derived(net != null && !Number.isNaN(net) && net > 80000);

	function edit(asset?: Schemas['AssetView']) {
		editing = asset;
		source = asset ? (asset.receipt_line_id != null ? 'receipt' : 'hand') : view?.candidates.length ? 'receipt' : 'hand';
		lineId = asset?.receipt_line_id ?? undefined;
		accountId = asset?.account_id;
		name = asset?.name ?? '';
		kind = asset?.kind ?? 'office';
		method = asset?.method ?? 'straight_line';
		life = String(asset?.life_years ?? 3);
		acquired = asset?.acquired ?? '';
		cost = asset ? (asset.cost / 100).toFixed(2).replace('.', ',') : '';
		formError = undefined;
		dialog = true;
	}

	function chooseLine() {
		if (candidate && !name) name = candidate.description;
		if (candidate) acquired = candidate.date;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body: Schemas['AssetInput'] = {
			name,
			kind,
			method,
			life_years: method === 'straight_line' ? Number(life) : null,
			receipt_line_id: source === 'receipt' ? (lineId ?? null) : null,
			account_id: source === 'hand' ? (accountId ?? null) : null,
			acquired: acquired || null,
			cost: source === 'hand' ? (parseMoney(cost, 'EUR') ?? null) : null
		};
		const result = editing
			? await call(api.PUT('/api/assets/{id}', { params: { path: { id: editing.id } }, body }))
			: await call(api.POST('/api/assets', { body }));
		formError = result.error;
		if (result.data) {
			dialog = false;
			toast(t('app.saved'));
			await load();
		}
	}

	async function remove(asset: Schemas['AssetView']) {
		if (!confirm(t('assets.delete_confirm'))) return;
		const result = await call(api.DELETE('/api/assets/{id}', { params: { path: { id: asset.id } } }));
		error = result.error;
		await load();
	}

	// disposal
	let disposing = $state<Schemas['AssetView']>();
	let disposalDate = $state(today());
	let disposalError = $state<ApiError>();
	let disposalOpen = $state(false);

	function dispose(asset: Schemas['AssetView']) {
		disposing = asset;
		disposalDate = today();
		disposalError = undefined;
		disposalOpen = true;
	}

	async function saveDisposal(event: SubmitEvent) {
		event.preventDefault();
		if (!disposing) return;
		const result = await call(
			api.POST('/api/assets/{id}/dispose', { params: { path: { id: disposing.id } }, body: { date: disposalDate } })
		);
		disposalError = result.error;
		if (result.data) {
			disposalOpen = false;
			toast(t('assets.disposed_toast'));
			await load();
		}
	}

	const groupRows = $derived(kinds.filter((k) => register?.groups[k]).map((k) => [k, register!.groups[k]] as const));
</script>

<div class="page-head">
	<div>
		<h1>{t('assets.title')}</h1>
		<p class="muted">{t('assets.intro')}</p>
	</div>
	{#if owner && view}
		<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('assets.new')}</button>
	{/if}
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if view?.due.length}
	<div class="card due">
		<p>{t('assets.due', { years: view.due.join(', ') })}</p>
		{#if owner}
			<button class="primary" onclick={() => bookDepreciation(view!.due[view!.due.length - 1])}>
				{t('assets.book_depreciation', { year: view.due[view.due.length - 1] })}
			</button>
		{/if}
	</div>
{/if}

{#if view}
	<div class="stack">
		<div class="row">
			<Segmented
				label={t('assets.title')}
				options={[
					{ value: 'assets', label: t('assets.list') },
					{ value: 'register', label: t('assets.register') }
				]}
				value={tab}
				onchange={(v) => (tab = v as typeof tab)}
			/>
			{#if tab === 'register'}
				<select class="year" aria-label={t('books.year')} bind:value={year}>
					{#each years as y (y)}<option value={y}>{y}</option>{/each}
				</select>
			{/if}
		</div>

		{#if tab === 'assets'}
			{#if view.assets.length}
				<ul class="assets">
					{#each view.assets as asset (asset.id)}
						<li class="card" class:disposed={asset.disposed}>
							<div class="head">
								<strong>{asset.name}</strong>
								<span class="badge">{t(`assets.kind.${asset.kind}`)}</span>
								{#if asset.method === 'low_value'}<span class="badge">{t('assets.low_value')}</span>{/if}
								{#if asset.disposed}<span class="badge">{t('assets.disposed_on', { date: day(asset.disposed) })}</span>{/if}
								{#if owner}
									<span class="push">
										<button class="ghost" onclick={() => edit(asset)}>{t('app.edit')}</button>
										{#if !asset.disposed}
											<button class="ghost" onclick={() => dispose(asset)}>{t('assets.dispose')}</button>
										{/if}
										{#if !asset.booked}
											<button class="ghost" onclick={() => remove(asset)}>{t('app.delete')}</button>
										{/if}
									</span>
								{/if}
							</div>
							<dl class="facts">
								<div><dt>{t('assets.acquired')}</dt><dd>{day(asset.acquired)}</dd></div>
								<div><dt>{t('assets.cost')}</dt><dd class="num">{eur(asset.cost)}</dd></div>
								<div>
									<dt>{t('assets.method')}</dt>
									<dd>
										{asset.method === 'low_value'
											? t('assets.method.low_value')
											: t('assets.life', { years: asset.life_years ?? 0 })}
									</dd>
								</div>
								<div><dt>{t('assets.book_value')}</dt><dd class="num">{eur(bookValue(asset))}</dd></div>
								<div>
									<dt>{t('term.ledger_account')}</dt>
									<dd>
										{accountName(asset.account_id)}
										{#if asset.receipt_id != null}· <a href="/receipts/{asset.receipt_id}">{t('term.receipt')}</a>{/if}
									</dd>
								</div>
							</dl>
							<table class="schedule">
								<thead>
									<tr>
										<th>{t('books.year')}</th>
										<th class="amount">{t('assets.depreciation')}</th>
										{#if asset.disposed}<th class="amount">{t('assets.disposal')}</th>{/if}
										<th class="amount">{t('assets.book_value_end')}</th>
										<th class="flag"><span class="visually-hidden">{t('assets.booked')}</span></th>
									</tr>
								</thead>
								<tbody>
									{#each asset.schedule as row (row.year)}
										<tr>
											<td>{row.year}</td>
											<td class="amount">{eur(row.depreciation)}</td>
											{#if asset.disposed}<td class="amount">{row.disposal ? eur(row.disposal) : '–'}</td>{/if}
											<td class="amount">{eur(row.book_value)}</td>
											<td class="flag">
												{#if row.booked}<span title={t('assets.booked')}><Icon name="check" size={16} /></span>{/if}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="card empty">{t('assets.none')}</p>
			{/if}
		{:else if register}
			<p class="muted">{t('assets.register_hint')}</p>
			{#if groupRows.length}
				<div class="table-wrap stacked">
					<table>
						<thead>
							<tr>
								<th>{t('assets.group')}</th>
								<th class="amount">{t('assets.cost')}</th>
								<th class="amount">{t('assets.start')}</th>
								<th class="amount">{t('assets.additions')}</th>
								<th class="amount">{t('assets.depreciation')}</th>
								<th class="amount">{t('assets.disposals')}</th>
								<th class="amount">{t('assets.end')}</th>
							</tr>
						</thead>
						<tbody>
							{#each groupRows as [k, g] (k)}
								<tr>
									<td data-label={t('assets.group')}>{t(`assets.kind.${k}`)}</td>
									<td data-label={t('assets.cost')} class="amount">{eur(g.cost)}</td>
									<td data-label={t('assets.start')} class="amount">{eur(g.start)}</td>
									<td data-label={t('assets.additions')} class="amount">{eur(g.additions)}</td>
									<td data-label={t('assets.depreciation')} class="amount">{eur(g.depreciation)}</td>
									<td data-label={t('assets.disposals')} class="amount">{eur(g.disposals)}</td>
									<td data-label={t('assets.end')} class="amount">{eur(g.end)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<p class="card empty">{t('assets.register_empty')}</p>
			{/if}
			{#if register.low_value.length}
				<h2>{t('assets.low_value_title')}</h2>
				<div class="table-wrap stacked">
					<table>
						<tbody>
							{#each register.low_value as asset (asset.id)}
								<tr>
									<td data-label={t('assets.name')}>{asset.name}</td>
									<td data-label={t('assets.acquired')}>{day(asset.acquired)}</td>
									<td data-label={t('assets.cost')} class="amount">{eur(asset.cost)}</td>
								</tr>
							{/each}
							<tr class="total">
								<td colspan="2">{t('assets.low_value_total')}</td>
								<td class="amount">{eur(register.low_value_total)}</td>
							</tr>
						</tbody>
					</table>
				</div>
			{/if}
		{/if}
	</div>
{/if}

<Dialog bind:open={dialog} title={t(editing ? 'assets.edit' : 'assets.new')}>
	<form class="stack" onsubmit={save}>
		{#if editing?.booked}<p class="muted">{t('assets.booked_hint')}</p>{/if}
		<Segmented
			label={t('assets.source')}
			options={[
				{ value: 'receipt', label: t('assets.from_receipt') },
				{ value: 'hand', label: t('assets.by_hand') }
			]}
			value={source}
			onchange={(v) => (source = v as typeof source)}
		/>
		{#if source === 'receipt'}
			<Field label={t('assets.receipt_line')} error={formError?.fields?.receipt_line_id}>
				<select bind:value={lineId} onchange={chooseLine}>
					<option value={undefined}>{t('assets.choose_line')}</option>
					{#each candidates as c (c.receipt_line_id)}
						<option value={c.receipt_line_id}>
							{day(c.date)} · {c.supplier_name ? `${c.supplier_name} · ` : ''}{c.description} · {eur(c.cost)}
						</option>
					{/each}
				</select>
			</Field>
			{#if !candidates.length}<small class="muted">{t('assets.no_lines')}</small>{/if}
		{:else}
			<Field label={t('term.ledger_account')} error={formError?.fields?.account_id}>
				<select bind:value={accountId}>
					<option value={undefined}>{t('books.choose_account')}</option>
					{#each accounts as a (a.id)}<option value={a.id}>{a.number} {a.name}</option>{/each}
				</select>
			</Field>
			<Field label={t('assets.cost')} error={formError?.fields?.cost} note={t('assets.cost_note')}>
				<input inputmode="decimal" bind:value={cost} />
			</Field>
		{/if}
		<Field label={t('assets.name')} error={formError?.fields?.name}>
			<input bind:value={name} />
		</Field>
		<div class="pair">
			<Field label={t('assets.group')}>
				<select bind:value={kind}>
					{#each kinds as k (k)}<option value={k}>{t(`assets.kind.${k}`)}</option>{/each}
				</select>
			</Field>
			<Field label={t('assets.acquired')} error={formError?.fields?.acquired}>
				<input type="date" bind:value={acquired} />
			</Field>
		</div>
		<Field
			label={t('assets.method')}
			error={formError?.fields?.method}
			note={overLimit ? t('assets.over_limit') : undefined}
		>
			<select bind:value={method}>
				<option value="straight_line">{t('assets.method.straight_line')}</option>
				<option value="low_value" disabled={overLimit}>{t('assets.method.low_value')}</option>
			</select>
		</Field>
		{#if method === 'straight_line'}
			<Field label={t('assets.life_years')} error={formError?.fields?.life_years} note={t('assets.life_note')}>
				<input type="number" min="1" max="50" bind:value={life} />
			</Field>
		{/if}
		{#if formError}<p class="alert" role="alert">{t(formError.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<Dialog bind:open={disposalOpen} title={t('assets.dispose_title', { name: disposing?.name ?? '' })}>
	<form class="stack" onsubmit={saveDisposal}>
		<p class="muted">{t('assets.dispose_hint')}</p>
		<Field label={t('assets.disposed')} error={disposalError?.fields?.date}>
			<input type="date" bind:value={disposalDate} />
		</Field>
		{#if disposalError}<p class="alert" role="alert">{t(disposalError.message)}</p>{/if}
		<div><button class="primary">{t('assets.dispose')}</button></div>
	</form>
</Dialog>

<style>
	.year {
		width: auto;
	}
	.due {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
		margin-bottom: 1rem;
		padding: 0.875rem 1rem;
	}
	.due p {
		margin: 0;
	}
	.assets {
		display: grid;
		gap: 0.75rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.assets li {
		padding: 0.875rem 1rem;
	}
	.assets li.disposed {
		opacity: 0.7;
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
		white-space: nowrap;
	}
	.facts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
		gap: 0.5rem 1rem;
		margin: 0 0 0.75rem;
	}
	.facts dt {
		color: var(--muted);
		font-size: 0.75rem;
	}
	.facts dd {
		margin: 0;
		overflow-wrap: anywhere;
	}
	.schedule {
		width: 100%;
		border-collapse: collapse;
	}
	.schedule th {
		color: var(--muted);
		font-size: 0.75rem;
		font-weight: 500;
		text-align: left;
	}
	.schedule td,
	.schedule th {
		padding: 0.25rem 0;
	}
	.amount {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.schedule td + td,
	.schedule th + th {
		padding-left: 0.75rem;
	}
	.num {
		font-variant-numeric: tabular-nums;
	}
	.schedule th.amount {
		text-align: right;
	}
	.flag {
		width: 2rem;
		text-align: right;
		color: var(--muted);
	}
	.total td {
		font-weight: 600;
	}
	h2 {
		margin-top: 1rem;
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.75rem;
	}
	@media (max-width: 40rem) {
		.pair {
			grid-template-columns: 1fr;
		}
	}
</style>
