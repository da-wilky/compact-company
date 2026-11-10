<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import CustomerForm from '#lib/components/CustomerForm.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';

	const owner = $derived(session.view?.user?.role === 'owner');
	let archived = $state(false);
	let customers = $state<Schemas['Customer'][]>([]);
	let search = $state('');
	let creating = $state(false);
	let error = $state<ApiError>();

	const shown = $derived(
		customers.filter((c) =>
			`${c.number} ${c.name}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase())
		)
	);

	async function load() {
		const result = await call(api.GET('/api/customers', { params: { query: { archived } } }));
		customers = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		archived;
		load();
	});
</script>

<div class="page-head">
	<h1>{t('customers.title')}</h1>
	{#if owner}<button class="primary" onclick={() => (creating = true)}>{t('customers.new')}</button>{/if}
</div>

<div class="stack">
	<div class="row">
		<Segmented
			label={t('customers.title')}
			options={[
				{ value: 'active', label: t('customers.active') },
				{ value: 'archived', label: t('customers.archived') }
			]}
			value={archived ? 'archived' : 'active'}
			onchange={(v) => (archived = v === 'archived')}
		/>
		<input class="search" type="search" placeholder={t('app.search')} aria-label={t('app.search')} bind:value={search} />
	</div>

	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	{#if shown.length}
		<div class="table-wrap stacked">
			<table>
				<thead>
					<tr>
						<th>{t('term.customer_number')}</th>
						<th>{t('customers.name')}</th>
						<th>{t('term.hourly_rate')}</th>
						<th>{t('customers.email')}</th>
					</tr>
				</thead>
				<tbody>
					{#each shown as customer (customer.id)}
						<tr>
							<td data-label={t('term.customer_number')}><span class="number">{customer.number}</span></td>
							<td data-label={t('customers.name')}><a href="/customers/{customer.id}">{customer.name}</a></td>
							<td data-label={t('term.hourly_rate')}>
								{customer.hourly_rate == null ? '–' : formatMoney(customer.hourly_rate, customer.currency)}
							</td>
							<td data-label={t('customers.email')}>{customer.email ?? '–'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<p class="card empty">{t(archived ? 'customers.none_archived' : 'customers.none')}</p>
	{/if}
</div>

<Dialog bind:open={creating} title={t('customers.new')}>
	{#if creating}
		<CustomerForm onsaved={(customer) => goto(`/customers/${customer.id}`)} />
	{/if}
</Dialog>

<style>
	.search {
		flex: 1 1 12rem;
		width: auto;
	}
	.number {
		font-variant-numeric: tabular-nums;
		color: var(--muted);
	}
	a {
		color: var(--text);
		font-weight: 500;
	}
</style>
