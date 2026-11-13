<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatAmount, kindLabel, kinds, type Kind } from '#lib/documents.ts';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';

	type Status = Schemas['DocumentStatus'];

	const owner = $derived(session.view?.user?.role === 'owner');
	let kind = $state<Kind | ''>('');
	let status = $state<Status | 'all'>('all');
	let documents = $state<Schemas['DocumentSummary'][]>([]);
	let error = $state<ApiError>();

	async function load() {
		const result = await call(
			api.GET('/api/documents', {
				params: {
					query: { kind: kind || undefined, status: status === 'all' ? undefined : status }
				}
			})
		);
		documents = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		kind;
		status;
		load();
	});

	let open = $state(false);
	let newKind = $state<Kind>('quote');
	let customerId = $state<number>();
	let projectId = $state<number>();
	let invoiceType = $state<Schemas['InvoiceType']>();
	let customers = $state<Schemas['Customer'][]>([]);
	let projects = $state<Schemas['Project'][]>([]);
	let createError = $state<ApiError>();

	async function start(kind: Kind) {
		newKind = kind;
		createError = undefined;
		open = true;
		if (!customers.length) customers = (await call(api.GET('/api/customers'))).data ?? [];
		customerId ??= customers[0]?.id;
	}

	$effect(() => {
		const customer_id = customerId;
		projectId = undefined;
		if (!customer_id) return;
		call(api.GET('/api/projects', { params: { query: { customer_id } } })).then(
			({ data }) => (projects = data ?? [])
		);
	});

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (!customerId) return;
		const result = await call(
			api.POST('/api/documents', {
				body: {
					kind: newKind,
					customer_id: customerId,
					project_id: projectId ?? null,
					invoice_type: newKind === 'invoice' ? (invoiceType ?? null) : null,
					lines: []
				}
			})
		);
		createError = result.error;
		if (result.data) goto(`/documents/${result.data.id}`);
	}

	const stateLabel = (state: Schemas['QuoteState']) => t(`documents.state.${state}`);
</script>

<div class="page-head">
	<h1>{t('documents.title')}</h1>
	{#if owner}
		<div class="actions">
			<a class="button ghost" href="/recurring"><Icon name="time" size={18} />{t('recurring.title')}</a>
			<button class="ghost" onclick={() => start('quote')}><Icon name="plus" size={18} />{t('term.quote')}</button>
			<button class="primary" onclick={() => start('invoice')}><Icon name="plus" size={18} />{t('term.invoice')}</button>
		</div>
	{/if}
</div>

<div class="stack">
	<div class="filters">
		<select aria-label={t('layouts.kind')} bind:value={kind}>
			<option value="">{t('documents.all_kinds')}</option>
			{#each kinds as k (k)}<option value={k}>{kindLabel(k)}</option>{/each}
		</select>
		<Segmented
			label={t('documents.status')}
			options={[
				{ value: 'all', label: t('documents.all') },
				{ value: 'draft', label: t('documents.drafts') },
				{ value: 'issued', label: t('documents.issued') }
			]}
			value={status}
			onchange={(v) => (status = v)}
		/>
	</div>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if documents.length}
		<ul class="list card-list">
			{#each documents as doc (doc.id)}
				<li>
					<a class="row" href={`/documents/${doc.id}`}>
						<span class="main">
							<strong>{doc.number ?? t('term.draft')}</strong>
							<small class="muted">
								{kindLabel(doc.kind)} · {doc.customer_name}{#if doc.title}&nbsp;· {doc.title}{/if}
							</small>
						</span>
						<span class="side">
							{#if doc.quote_state}
								<small class="badge" class:accent={doc.quote_state === 'accepted'}>{stateLabel(doc.quote_state)}</small>
							{/if}
							{#if doc.gross != null}<span class="amount">{formatAmount(doc.gross, doc.currency)}</span>{/if}
							<small class="muted">{doc.issue_date ? formatDay(doc.issue_date, { dateStyle: 'medium' }) : ''}</small>
						</span>
					</a>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="card empty">{t('documents.none')}</p>
	{/if}
</div>

<Dialog bind:open title={t('documents.new', { kind: kindLabel(newKind) })}>
	<form class="stack" onsubmit={create}>
		<Field label={t('layouts.kind')}>
			<select bind:value={newKind}>
				{#each kinds.filter((k) => k !== 'cancellation' && k !== 'dunning') as k (k)}<option value={k}>{kindLabel(k)}</option>{/each}
			</select>
		</Field>
		<Field label={t('term.customer')} error={createError?.fields?.customer_id}>
			<select bind:value={customerId}>
				{#each customers as customer (customer.id)}
					<option value={customer.id}>{customer.name}</option>
				{/each}
			</select>
		</Field>
		<Field label={t('term.project')}>
			<select bind:value={projectId}>
				<option value={undefined}>–</option>
				{#each projects as project (project.id)}
					<option value={project.id}>{project.name}</option>
				{/each}
			</select>
		</Field>
		{#if newKind === 'invoice'}
			<Field label={t('documents.invoice_type')} note={t('documents.invoice_type_hint')}>
				<select bind:value={invoiceType}>
					<option value={undefined}>{t('term.invoice')}</option>
					<option value="partial">{t('term.partial_invoice')}</option>
					<option value="final">{t('term.final_invoice')}</option>
				</select>
			</Field>
		{/if}
		{#if createError}<p class="alert" role="alert">{t(createError.message)}</p>{/if}
		<div><button class="primary" disabled={!customerId}>{t('documents.create')}</button></div>
	</form>
</Dialog>

<style>
	.actions,
	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: center;
	}
	.card-list {
		max-width: 64rem;
		background: var(--surface);
	}
	.card-list li {
		padding: 0;
	}
	.row {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
		padding: 0.875rem 1rem;
		color: inherit;
		text-decoration: none;
		width: 100%;
		min-width: 0;
	}
	.row:hover {
		background: var(--surface-2);
	}
	.main {
		display: grid;
		min-width: 0;
	}
	.main small {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.side {
		display: flex;
		flex-wrap: wrap;
		justify-content: end;
		align-items: center;
		gap: 0.25rem 0.75rem;
		text-align: right;
	}
	.amount {
		font-variant-numeric: tabular-nums;
		font-weight: 500;
	}
</style>
