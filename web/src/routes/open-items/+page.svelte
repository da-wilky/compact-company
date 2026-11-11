<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';

	const owner = $derived(session.view?.user?.role === 'owner');
	let items = $state<Schemas['OpenItems']>();
	let error = $state<ApiError>();

	call(api.GET('/api/open-items')).then((result) => {
		items = result.data;
		error = result.error;
	});

	const day = (iso: string | null | undefined) => (iso ? formatDay(iso, { dateStyle: 'medium' }) : '–');

	async function dun(invoice: Schemas['OpenInvoiceItem']) {
		if (invoice.dunning_draft_id != null) return goto(`/documents/${invoice.dunning_draft_id}`);
		const result = await call(
			api.POST('/api/documents/{id}/follow-up', { params: { path: { id: invoice.id } }, body: { kind: 'dunning' } })
		);
		error = result.error;
		if (result.data) goto(`/documents/${result.data.id}`);
	}

	const totals = (list: { due: number; currency: string }[]) =>
		Object.entries(
			list.reduce<Record<string, number>>((sum, i) => ({ ...sum, [i.currency]: (sum[i.currency] ?? 0) + i.due }), {})
		)
			.map(([currency, minor]) => formatMoney(minor, currency))
			.join(' · ');
</script>

<div class="page-head">
	<div>
		<h1>{t('open_items.title')}</h1>
		<p class="muted">{t('open_items.intro')}</p>
	</div>
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if items}
	<div class="stack">
		<section class="stack">
			<div class="row heading">
				<h2>{t('open_items.receivable')}</h2>
				{#if items.invoices.length}
					<span class="muted">{totals(items.invoices.map((i) => ({ due: i.due.minor, currency: i.due.currency })))}</span>
				{/if}
			</div>
			{#if items.invoices.length}
				<div class="table-wrap stacked">
					<table>
						<thead>
							<tr>
								<th>{t('term.invoice')}</th>
								<th>{t('term.customer')}</th>
								<th>{t('documents.due_date')}</th>
								<th class="amount">{t('documents.amount_due')}</th>
								<th>{t('term.dunning_letter')}</th>
								{#if owner}<th></th>{/if}
							</tr>
						</thead>
						<tbody>
							{#each items.invoices as invoice (invoice.id)}
								<tr>
									<td data-label={t('term.invoice')}><a href="/documents/{invoice.id}">{invoice.number}</a></td>
									<td data-label={t('term.customer')}>{invoice.customer_name}</td>
									<td data-label={t('documents.due_date')}>
										{day(invoice.due_date)}
										{#if invoice.overdue_days > 0}<span class="badge danger">{t('open_items.overdue_days', { days: String(invoice.overdue_days) })}</span>{/if}
									</td>
									<td data-label={t('documents.amount_due')} class="amount">{formatMoney(invoice.due.minor, invoice.due.currency)}</td>
									<td data-label={t('term.dunning_letter')}>
										{#if invoice.dunning_level}
											{t(`documents.dunning_${invoice.dunning_level}`)} · {day(invoice.dunned_on)}
										{:else}–{/if}
									</td>
									{#if owner}
										<td class="actions">
											{#if invoice.dunning_draft_id != null}
												<button class="ghost" onclick={() => dun(invoice)}>{t('open_items.dunning_draft')}</button>
											{:else if (invoice.dunning_level ?? 0) < 3}
												<button class="ghost" onclick={() => dun(invoice)}>
													{t(`open_items.dun_${(invoice.dunning_level ?? 0) + 1}`)}
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
				<p class="card empty">{t('open_items.no_invoices')}</p>
			{/if}
		</section>

		<section class="stack">
			<div class="row heading">
				<h2>{t('open_items.payable')}</h2>
				{#if items.receipts.length}<span class="muted">{totals(items.receipts)}</span>{/if}
			</div>
			{#if items.receipts.length}
				<div class="table-wrap stacked">
					<table>
						<thead>
							<tr>
								<th>{t('term.receipt')}</th>
								<th>{t('receipts.date')}</th>
								<th>{t('documents.due_date')}</th>
								<th class="amount">{t('documents.amount_due')}</th>
							</tr>
						</thead>
						<tbody>
							{#each items.receipts as receipt (receipt.id)}
								<tr>
									<td data-label={t('term.receipt')}>
										<a href="/receipts/{receipt.id}">{receipt.supplier_name}{receipt.number ? ` · ${receipt.number}` : ''}</a>
									</td>
									<td data-label={t('receipts.date')}>{day(receipt.date)}</td>
									<td data-label={t('documents.due_date')}>
										{day(receipt.due_date)}
										{#if receipt.overdue_days > 0}<span class="badge danger">{t('open_items.overdue_days', { days: String(receipt.overdue_days) })}</span>{/if}
									</td>
									<td data-label={t('documents.amount_due')} class="amount">{formatMoney(receipt.due, receipt.currency)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<p class="card empty">{t('open_items.no_receipts')}</p>
			{/if}
		</section>
	</div>
{/if}

<style>
	.heading {
		align-items: baseline;
		justify-content: space-between;
	}
	.heading h2 {
		margin: 0;
	}
	.amount {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.actions {
		text-align: right;
		white-space: nowrap;
	}
	.badge {
		margin-left: 0.375rem;
	}
</style>
