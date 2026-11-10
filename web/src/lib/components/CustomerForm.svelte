<script lang="ts">
	import { untrack } from 'svelte';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { moneyInput, parseMoney } from '#lib/money.ts';
	import Field from './Field.svelte';

	let {
		customer,
		onsaved
	}: { customer?: Schemas['Customer']; onsaved: (customer: Schemas['Customer']) => void } =
		$props();

	// the form edits a copy; later changes to the prop do not reset what the user typed
	const initial = untrack(() => customer);
	let name = $state(initial?.name ?? '');
	let address = $state(initial?.address ?? '');
	let email = $state(initial?.email ?? '');
	let language = $state<Schemas['Language']>(initial?.language ?? 'de');
	let currency = $state(initial?.currency ?? '');
	let rate = $state(initial ? moneyInput(initial.hourly_rate, initial.currency) : '');
	let note = $state(initial?.note ?? '');
	let currencies = $state<string[]>([]);
	let error = $state<ApiError>();

	call(api.GET('/api/currencies')).then(({ data }) => {
		currencies = data ?? [];
		if (!currency) currency = currencies[0] ?? 'EUR';
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const hourly_rate = parseMoney(rate, currency);
		if (Number.isNaN(hourly_rate)) {
			error = { code: 'invalid_input', message: 'error.invalid_input', fields: { hourly_rate: 'invalid_format' } };
			return;
		}
		const body: Schemas['CustomerInput'] = {
			name,
			address,
			email,
			language,
			currency,
			hourly_rate,
			note,
			archived: initial?.archived ?? false
		};
		const result = initial
			? await call(api.PUT('/api/customers/{id}', { params: { path: { id: initial.id } }, body }))
			: await call(api.POST('/api/customers', { body }));
		error = result.error;
		if (result.data) onsaved(result.data);
	}
</script>

<form class="grid" onsubmit={save}>
	<Field label={t('customers.name')} error={error?.fields?.name}>
		<input bind:value={name} autocomplete="organization" />
	</Field>
	<Field label={t('customers.email')} error={error?.fields?.email}>
		<input type="email" bind:value={email} autocomplete="email" />
	</Field>
	<div class="wide">
		<Field label={t('customers.address')} error={error?.fields?.address}>
			<textarea rows="3" bind:value={address}></textarea>
		</Field>
	</div>
	<Field label={t('customers.language')}>
		<select bind:value={language}>
			<option value="de">{t('language.de')}</option>
			<option value="en">{t('language.en')}</option>
		</select>
	</Field>
	<Field label={t('customers.currency')} error={error?.fields?.currency}>
		<select bind:value={currency}>
			{#each currencies as code (code)}<option value={code}>{code}</option>{/each}
		</select>
	</Field>
	<Field label={t('term.hourly_rate')} error={error?.fields?.hourly_rate} note={currency}>
		<input inputmode="decimal" bind:value={rate} />
	</Field>
	<div class="wide">
		<Field label={t('customers.note')} error={error?.fields?.note}>
			<textarea rows="2" bind:value={note}></textarea>
		</Field>
	</div>
	{#if error}<p class="alert wide" role="alert">{t(error.message)}</p>{/if}
	<div class="wide"><button class="primary">{t('app.save')}</button></div>
</form>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: 1rem 1.25rem;
	}
	.wide {
		grid-column: 1 / -1;
	}
</style>
