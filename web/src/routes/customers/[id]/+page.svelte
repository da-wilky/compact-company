<script lang="ts">
	import { page } from '$app/state';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import CustomerForm from '#lib/components/CustomerForm.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	const id = $derived(Number(page.params.id));
	const owner = $derived(session.view?.user?.role === 'owner');
	let customer = $state<Schemas['Customer']>();
	let editing = $state(false);
	let error = $state<ApiError>();

	$effect(() => {
		call(api.GET('/api/customers/{id}', { params: { path: { id } } })).then((result) => {
			customer = result.data;
			error = result.error;
		});
	});

	async function setArchived(archived: boolean) {
		if (!customer) return;
		const { id: _, number: __, created_at: ___, ...fields } = customer;
		const result = await call(
			api.PUT('/api/customers/{id}', { params: { path: { id } }, body: { ...fields, archived } })
		);
		error = result.error;
		if (result.data) {
			customer = result.data;
			toast(t(archived ? 'customers.archived_done' : 'customers.restored'));
		}
	}

	let projects = $state<Schemas['Project'][]>([]);
	let showArchived = $state(false);
	let editingProject = $state<Schemas['Project']>();
	let projectDialog = $state(false);

	async function loadProjects() {
		const result = await call(
			api.GET('/api/projects', { params: { query: { customer_id: id, archived: showArchived } } })
		);
		projects = result.data ?? [];
	}

	$effect(() => {
		showArchived;
		loadProjects();
	});

	function editProject(project?: Schemas['Project']) {
		editingProject = project;
		projectDialog = true;
	}

	async function projectSaved() {
		projectDialog = false;
		toast(t('app.saved'));
		await loadProjects();
	}

	async function setProjectArchived(project: Schemas['Project'], archived: boolean) {
		const body = {
			customer_id: project.customer_id,
			name: project.name,
			hourly_rate: project.hourly_rate,
			billable: project.billable,
			archived
		};
		const result = await call(
			api.PUT('/api/projects/{id}', { params: { path: { id: project.id } }, body })
		);
		error = result.error;
		await loadProjects();
	}

	function saved(updated: Schemas['Customer']) {
		customer = updated;
		editing = false;
		toast(t('app.saved'));
	}
</script>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

{#if customer}
	<div class="page-head">
		<div>
			<p class="muted"><a href="/customers">{t('customers.title')}</a> · {customer.number}</p>
			<h1>{customer.name}</h1>
		</div>
		{#if customer.archived}<span class="badge">{t('term.archived')}</span>{/if}
		{#if owner}
			<div class="row">
				<button onclick={() => (editing = !editing)}>{t(editing ? 'app.cancel' : 'app.edit')}</button>
				<button onclick={() => setArchived(!customer?.archived)}>
					{t(customer.archived ? 'customers.restore' : 'customers.archive')}
				</button>
			</div>
		{/if}
	</div>

	<section class="card">
		{#if editing}
			<CustomerForm {customer} onsaved={saved} />
		{:else}
			<dl>
				<dt>{t('term.hourly_rate')}</dt>
				<dd>{customer.hourly_rate == null ? '–' : formatMoney(customer.hourly_rate, customer.currency)}</dd>
				<dt>{t('customers.email')}</dt>
				<dd>{customer.email ?? '–'}</dd>
				<dt>{t('customers.address')}</dt>
				<dd class="pre">{[customer.street, [customer.postcode, customer.city].filter(Boolean).join(' '), customer.country]
						.filter(Boolean)
						.join('\n')}</dd>
				<dt>{t('customers.language')}</dt>
				<dd>{t(`language.${customer.language}`)}</dd>
				<dt>{t('customers.currency')}</dt>
				<dd>{customer.currency}</dd>
				{#if customer.note}
					<dt>{t('customers.note')}</dt>
					<dd class="pre">{customer.note}</dd>
				{/if}
			</dl>
		{/if}
	</section>

	<section class="card stack">
		<div class="row">
			<h2>{t('projects.title')}</h2>
			<label class="row toggle">
				<input type="checkbox" bind:checked={showArchived} />{t('app.show_archived')}
			</label>
			{#if owner && !customer.archived}
				<button class="primary" onclick={() => editProject()}><Icon name="plus" size={18} />{t('projects.new')}</button>
			{/if}
		</div>
		{#if projects.length}
			<ul class="list">
				{#each projects as project (project.id)}
					<li>
						<div>
							<strong>{project.name}</strong>
							<small class="muted">
								{project.hourly_rate == null
									? t('projects.uses_customer_rate')
									: formatMoney(project.hourly_rate, project.currency)}
								· {t(project.billable ? 'projects.billable' : 'projects.not_billable')}
							</small>
						</div>
						{#if owner}
							<button class="ghost" onclick={() => editProject(project)}>{t('app.edit')}</button>
							<button class="ghost" onclick={() => setProjectArchived(project, !project.archived)}>
								{t(project.archived ? 'customers.restore' : 'customers.archive')}
							</button>
						{/if}
					</li>
				{/each}
			</ul>
		{:else}
			<p class="muted">{t('projects.none')}</p>
		{/if}
	</section>

	<Dialog bind:open={projectDialog} title={t(editingProject ? 'projects.edit' : 'projects.new')}>
		{#if projectDialog}
			<ProjectForm {customer} project={editingProject} onsaved={projectSaved} />
		{/if}
	</Dialog>
{/if}

<style>
	.muted a {
		color: inherit;
	}
	section {
		max-width: 48rem;
	}
	section + section {
		margin-top: 1rem;
	}
	.row h2 {
		margin-right: auto;
	}
	.toggle {
		color: var(--muted);
		font-size: 0.8125rem;
	}
	dl {
		display: grid;
		grid-template-columns: minmax(8rem, max-content) 1fr;
		gap: 0.625rem 1.5rem;
		margin: 0;
	}
	dt {
		color: var(--muted);
	}
	dd {
		margin: 0;
	}
	.pre {
		white-space: pre-line;
	}
</style>
