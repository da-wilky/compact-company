<script lang="ts">
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import { toast } from '#lib/toast.svelte.ts';

	let archived = $state(false);
	let activities = $state<Schemas['Activity'][]>([]);
	let currencies = $state<string[]>([]);
	let open = $state(false);
	let editing = $state<Schemas['Activity']>();
	let name = $state('');
	let rates = $state<{ currency: string; amount: string }[]>([]);
	let error = $state<ApiError>();

	async function load() {
		const result = await call(api.GET('/api/activities', { params: { query: { archived } } }));
		activities = result.data ?? [];
		error = result.error;
	}

	$effect(() => {
		archived;
		load();
	});
	call(api.GET('/api/currencies')).then(({ data }) => (currencies = data ?? []));

	function edit(activity?: Schemas['Activity']) {
		editing = activity;
		name = activity?.name ?? '';
		rates = (activity?.rates ?? []).map((r) => ({
			currency: r.currency,
			amount: moneyInput(r.minor, r.currency)
		}));
		if (!rates.length) rates = [{ currency: currencies[0] ?? 'EUR', amount: '' }];
		error = undefined;
		open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const parsed = rates
			.filter((r) => r.amount.trim())
			.map((r) => ({ currency: r.currency, minor: parseMoney(r.amount, r.currency) }));
		if (parsed.some((r) => r.minor == null || Number.isNaN(r.minor))) {
			error = { ...clientError('error.invalid_input'), fields: { rates: 'invalid_format' } };
			return;
		}
		const body: Schemas['ActivityInput'] = {
			name,
			rates: parsed as Schemas['Money'][],
			archived: editing?.archived ?? false
		};
		const result = editing
			? await call(api.PUT('/api/activities/{id}', { params: { path: { id: editing.id } }, body }))
			: await call(api.POST('/api/activities', { body }));
		error = result.error;
		if (result.data) {
			open = false;
			toast(t('app.saved'));
			await load();
		}
	}

	async function setArchived(activity: Schemas['Activity'], archived: boolean) {
		const body = { name: activity.name, rates: activity.rates, archived };
		const result = await call(
			api.PUT('/api/activities/{id}', { params: { path: { id: activity.id } }, body })
		);
		error = result.error;
		await load();
	}
</script>

<div class="page-head">
	<div>
		<h1>{t('activities.title')}</h1>
		<p class="muted">{t('activities.intro')}</p>
	</div>
	<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('activities.new')}</button>
</div>

<div class="stack">
	<div>
		<Segmented
			label={t('activities.title')}
			options={[
				{ value: 'active', label: t('app.active') },
				{ value: 'archived', label: t('app.archived') }
			]}
			value={archived ? 'archived' : 'active'}
			onchange={(v) => (archived = v === 'archived')}
		/>
	</div>
	{#if error && !open}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if activities.length}
		<ul class="list card-list">
			{#each activities as activity (activity.id)}
				<li>
					<div>
						<strong>{activity.name}</strong>
						<small class="muted">
							{activity.rates.length
								? activity.rates.map((r) => formatMoney(r.minor, r.currency)).join(' · ')
								: t('activities.no_rates')}
						</small>
					</div>
					<button class="ghost" onclick={() => edit(activity)}>{t('app.edit')}</button>
					<button class="ghost" onclick={() => setArchived(activity, !activity.archived)}>
						{t(activity.archived ? 'customers.restore' : 'customers.archive')}
					</button>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="card empty">{t('activities.none')}</p>
	{/if}
</div>

<Dialog bind:open title={t(editing ? 'activities.edit' : 'activities.new')}>
	<form class="stack" onsubmit={save}>
		<Field label={t('activities.name')} error={error?.fields?.name}>
			<input bind:value={name} />
		</Field>
		<fieldset class="stack">
			<legend>{t('activities.rates')}</legend>
			{#each rates as rate, i (i)}
				<div class="rate">
					<select aria-label={t('customers.currency')} bind:value={rate.currency}>
						{#each currencies as code (code)}<option value={code}>{code}</option>{/each}
					</select>
					<input aria-label={t('term.hourly_rate')} inputmode="decimal" bind:value={rate.amount} />
					<button type="button" class="ghost icon" aria-label={t('app.remove')} onclick={() => rates.splice(i, 1)}>
						<Icon name="close" size={18} />
					</button>
				</div>
			{/each}
			<div>
				<button type="button" class="ghost" onclick={() => rates.push({ currency: currencies[0] ?? 'EUR', amount: '' })}>
					<Icon name="plus" size={18} />{t('activities.add_rate')}
				</button>
			</div>
			<small class="muted">{t('activities.rate_hint')}</small>
			{#if error?.fields?.rates}<small class="error">{t(`field.${error.fields.rates}`)}</small>{/if}
		</fieldset>
		{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
		<div><button class="primary">{t('app.save')}</button></div>
	</form>
</Dialog>

<style>
	.card-list {
		max-width: 48rem;
		background: var(--surface);
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		gap: 0.5rem;
	}
	legend {
		margin-bottom: 0.375rem;
		font-weight: 500;
	}
	.rate {
		display: grid;
		grid-template-columns: 6rem 1fr auto;
		gap: 0.5rem;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
</style>
