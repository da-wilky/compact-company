<script lang="ts">
	import { untrack } from 'svelte';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import Field from './Field.svelte';

	let {
		customer,
		project,
		onsaved
	}: {
		customer: Schemas['Customer'];
		project?: Schemas['Project'];
		onsaved: (project: Schemas['Project']) => void;
	} = $props();

	// the form edits a copy; later changes to the props do not reset what the user typed
	const initial = untrack(() => project);
	const currency = untrack(() => customer.currency);
	let name = $state(initial?.name ?? '');
	let rate = $state(moneyInput(initial?.hourly_rate, currency));
	let billable = $state(initial?.billable ?? true);
	let error = $state<ApiError>();

	const fallback = $derived(
		customer.hourly_rate == null
			? t('projects.no_rate')
			: t('projects.customer_rate', { rate: formatMoney(customer.hourly_rate, currency) })
	);

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const hourly_rate = parseMoney(rate, currency);
		if (Number.isNaN(hourly_rate)) {
			error = { ...clientError('error.invalid_input'), fields: { hourly_rate: 'invalid_format' } };
			return;
		}
		const body: Schemas['ProjectInput'] = {
			customer_id: customer.id,
			name,
			hourly_rate,
			billable,
			archived: initial?.archived ?? false
		};
		const result = initial
			? await call(api.PUT('/api/projects/{id}', { params: { path: { id: initial.id } }, body }))
			: await call(api.POST('/api/projects', { body }));
		error = result.error;
		if (result.data) onsaved(result.data);
	}
</script>

<form class="stack" onsubmit={save}>
	<Field label={t('projects.name')} error={error?.fields?.name}>
		<input bind:value={name} />
	</Field>
	<Field label={`${t('term.hourly_rate')} (${currency})`} error={error?.fields?.hourly_rate} note={fallback}>
		<input inputmode="decimal" bind:value={rate} />
	</Field>
	<Field label={t('projects.billable')} inline>
		<input type="checkbox" bind:checked={billable} />
	</Field>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	<div><button class="primary">{t('app.save')}</button></div>
</form>
