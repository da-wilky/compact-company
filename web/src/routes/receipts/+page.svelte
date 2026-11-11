<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, call, upload, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';

	type Filter = 'all' | 'draft' | 'open';

	const owner = $derived(session.view?.user?.role === 'owner');
	let filter = $state<Filter>('all');
	let receipts = $state<Schemas['ReceiptSummary'][]>([]);
	let error = $state<ApiError>();
	let busy = $state(false);
	let input = $state<HTMLInputElement>();

	async function load() {
		const query = filter === 'draft' ? { status: 'draft' as const } : filter === 'open' ? { open: true } : {};
		const result = await call(api.GET('/api/receipts', { params: { query } }));
		receipts = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		filter;
		load();
	});

	async function create(file?: File) {
		const form = new FormData();
		if (file) form.append('file', file);
		busy = true;
		const result = await upload<Schemas['Receipt']>('POST', '/api/receipts', form);
		busy = false;
		error = result.error;
		if (result.data) goto(`/receipts/${result.data.id}`);
	}

	function picked(event: Event) {
		const file = (event.currentTarget as HTMLInputElement).files?.[0];
		if (file) create(file);
	}

	let dragging = $state(false);
	function dropped(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		const file = event.dataTransfer?.files[0];
		if (file && owner) create(file);
	}

	const day = (iso: string | null | undefined) => (iso ? formatDay(iso, { dateStyle: 'medium' }) : '–');
</script>

<div class="page-head">
	<div>
		<h1>{t('receipts.title')}</h1>
		<p class="muted">{t('receipts.intro')}</p>
	</div>
	{#if owner}
		<div class="row">
			<button class="ghost" onclick={() => create()} disabled={busy}>{t('receipts.without_file')}</button>
			<button class="primary" onclick={() => input?.click()} disabled={busy}>
				<Icon name="plus" size={18} />{t('receipts.upload')}
			</button>
			<input bind:this={input} type="file" hidden accept="application/pdf,image/jpeg,image/png,image/webp,.xml,text/xml,application/xml" onchange={picked} />
		</div>
	{/if}
</div>

<div
	class="stack"
	role="region"
	aria-label={t('receipts.title')}
	class:dragging
	ondragover={(e) => {
		if (!owner) return;
		e.preventDefault();
		dragging = true;
	}}
	ondragleave={() => (dragging = false)}
	ondrop={dropped}
>
	<div>
		<Segmented
			label={t('receipts.title')}
			options={[
				{ value: 'all', label: t('receipts.all') },
				{ value: 'draft', label: t('receipts.drafts') },
				{ value: 'open', label: t('receipts.open') }
			]}
			value={filter}
			onchange={(v) => (filter = v as Filter)}
		/>
	</div>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if receipts.length}
		<div class="table-wrap stacked">
			<table>
				<thead>
					<tr>
						<th>{t('receipts.date')}</th>
						<th>{t('term.supplier')}</th>
						<th>{t('receipts.number')}</th>
						<th class="amount">{t('receipts.gross')}</th>
						<th class="amount">{t('documents.amount_due')}</th>
					</tr>
				</thead>
				<tbody>
					{#each receipts as receipt (receipt.id)}
						<tr>
							<td data-label={t('receipts.date')}>{day(receipt.date)}</td>
							<td data-label={t('term.supplier')}>
								<a href="/receipts/{receipt.id}">{receipt.supplier_name ?? t('receipts.no_supplier')}</a>
								{#if receipt.status === 'draft'}<span class="badge">{t('term.draft')}</span>{/if}
								{#if receipt.reversed}<span class="badge">{t('books.is_reversed')}</span>{/if}
								{#if receipt.format}<span class="badge accent">{t('receipts.e_invoice')}</span>{/if}
							</td>
							<td data-label={t('receipts.number')}>{receipt.number ?? '–'}</td>
							<td data-label={t('receipts.gross')} class="amount">{formatMoney(receipt.gross, receipt.currency)}</td>
							<td data-label={t('documents.amount_due')} class="amount">
								{receipt.amount_due == null ? '–' : formatMoney(receipt.amount_due, receipt.currency)}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<p class="card empty">{t(owner ? 'receipts.none_drop' : 'receipts.none')}</p>
	{/if}
</div>

<style>
	.dragging {
		outline: 2px dashed var(--accent);
		outline-offset: 0.5rem;
		border-radius: var(--radius);
	}
	.amount {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.badge {
		margin-left: 0.375rem;
	}
</style>
