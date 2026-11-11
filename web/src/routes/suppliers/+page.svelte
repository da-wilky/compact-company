<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Input = Schemas['SupplierInput'];

	const owner = $derived(session.view?.user?.role === 'owner');
	let archived = $state(false);
	let suppliers = $state<Schemas['Supplier'][]>([]);
	let accounts = $state<Schemas['Account'][]>([]);
	let open = $state(false);
	let editing = $state<Schemas['Supplier']>();
	let values = $state<Input>({});
	let error = $state<ApiError>();

	async function load() {
		const result = await call(api.GET('/api/suppliers', { params: { query: { archived } } }));
		suppliers = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		archived;
		load();
	});
	call(api.GET('/api/accounts')).then(({ data }) => (accounts = data ?? []));

	const accountName = (id: number | null | undefined) => {
		const account = accounts.find((a) => a.id === id);
		return account ? `${account.number} ${account.name}` : '–';
	};

	function edit(supplier?: Schemas['Supplier']) {
		editing = supplier;
		values = supplier ? { ...supplier } : { country: 'DE' };
		error = undefined;
		open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body: Input = {
			...Object.fromEntries(Object.entries(values).map(([k, v]) => [k, v === '' ? null : v])),
			archived: editing?.archived ?? false
		};
		delete (body as Record<string, unknown>).id;
		delete (body as Record<string, unknown>).created_at;
		const result = editing
			? await call(api.PUT('/api/suppliers/{id}', { params: { path: { id: editing.id } }, body }))
			: await call(api.POST('/api/suppliers', { body }));
		error = result.error;
		if (result.data) {
			open = false;
			toast(t('app.saved'));
			await load();
		}
	}

	async function setArchived(supplier: Schemas['Supplier'], archived: boolean) {
		const { id, created_at, ...rest } = supplier;
		const result = await call(api.PUT('/api/suppliers/{id}', { params: { path: { id } }, body: { ...rest, archived } }));
		error = result.error;
		await load();
	}

	const fields: { key: 'name' | 'street' | 'postcode' | 'city' | 'country' | 'vat_id' | 'iban' | 'email'; label: string }[] = $derived([
		{ key: 'name', label: t('customers.name') },
		{ key: 'street', label: t('customers.street') },
		{ key: 'postcode', label: t('customers.postcode') },
		{ key: 'city', label: t('customers.city') },
		{ key: 'country', label: t('customers.country') },
		{ key: 'vat_id', label: t('business.vat_id') },
		{ key: 'iban', label: t('business.iban') },
		{ key: 'email', label: t('customers.email') }
	]);
</script>

<div class="page-head">
	<div>
		<h1>{t('suppliers.title')}</h1>
		<p class="muted">{t('suppliers.intro')}</p>
	</div>
	{#if owner}
		<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('suppliers.new')}</button>
	{/if}
</div>

<div class="stack">
	<div>
		<Segmented
			label={t('suppliers.title')}
			options={[
				{ value: 'active', label: t('app.active') },
				{ value: 'archived', label: t('app.archived') }
			]}
			value={archived ? 'archived' : 'active'}
			onchange={(v) => (archived = v === 'archived')}
		/>
	</div>
	{#if error && !open}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if suppliers.length}
		<div class="table-wrap stacked">
			<table>
				<thead>
					<tr>
						<th>{t('customers.name')}</th>
						<th>{t('customers.city')}</th>
						<th>{t('business.vat_id')}</th>
						<th>{t('suppliers.account')}</th>
						{#if owner}<th></th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each suppliers as supplier (supplier.id)}
						<tr>
							<td data-label={t('customers.name')}><strong>{supplier.name}</strong></td>
							<td data-label={t('customers.city')}>{supplier.city ?? '–'}</td>
							<td data-label={t('business.vat_id')}>{supplier.vat_id ?? '–'}</td>
							<td data-label={t('suppliers.account')}>{accountName(supplier.account_id)}</td>
							{#if owner}
								<td class="actions">
									<button class="ghost" onclick={() => edit(supplier)}>{t('app.edit')}</button>
									<button class="ghost" onclick={() => setArchived(supplier, !supplier.archived)}>
										{t(supplier.archived ? 'customers.restore' : 'customers.archive')}
									</button>
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<p class="card empty">{t('suppliers.none')}</p>
	{/if}
</div>

<Dialog bind:open title={t(editing ? 'suppliers.edit' : 'suppliers.new')}>
	<form class="stack" onsubmit={save}>
		<div class="grid">
			{#each fields as field (field.key)}
				<Field label={field.label} error={error?.fields?.[field.key]}>
					<input bind:value={values[field.key]} />
				</Field>
			{/each}
		</div>
		<Field label={t('suppliers.account')} note={t('suppliers.account_note')} error={error?.fields?.account_id}>
			<select bind:value={values.account_id}>
				<option value={null}>–</option>
				{#each accounts as account (account.id)}
					<option value={account.id}>{account.number} {account.name}</option>
				{/each}
			</select>
		</Field>
		{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
		gap: 0.75rem;
	}
	.actions {
		text-align: right;
		white-space: nowrap;
	}
</style>
