<script lang="ts">
	import { untrack } from 'svelte';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { moneyInput, parseMoney } from '#lib/money.ts';
	import Field from './Field.svelte';
	import Icon from './Icon.svelte';

	let {
		customer,
		onsaved
	}: { customer?: Schemas['Customer']; onsaved: (customer: Schemas['Customer']) => void } =
		$props();

	// the form edits a copy; later changes to the prop do not reset what the user typed
	const initial = untrack(() => customer);
	let name = $state(initial?.name ?? '');
	type EditAddress = Omit<Schemas['AddressInput'], 'id'> & { id?: number };
	const blank = (is_default: boolean): EditAddress => ({
		label: '',
		street: '',
		postcode: '',
		city: '',
		country: 'DE',
		is_default
	});
	let addresses = $state<EditAddress[]>(initial?.addresses.map((a) => ({ ...a })) ?? [blank(true)]);

	function removeAddress(i: number) {
		const [gone] = addresses.splice(i, 1);
		if (gone.is_default) addresses[0].is_default = true;
	}

	function makeDefault(i: number) {
		addresses.forEach((a, j) => (a.is_default = i === j));
	}
	let email = $state(initial?.email ?? '');
	let language = $state<Schemas['Language']>(initial?.language ?? 'de');
	let currency = $state(initial?.currency ?? '');
	let rate = $state(initial ? moneyInput(initial.hourly_rate, initial.currency) : '');
	let note = $state(initial?.note ?? '');
	let vatId = $state(initial?.vat_id ?? '');
	let leitwegId = $state(initial?.leitweg_id ?? '');
	let xrechnung = $state(initial?.xrechnung ?? false);
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
			error = { ...clientError('error.invalid_input'), fields: { hourly_rate: 'invalid_format' } };
			return;
		}
		const body: Schemas['CustomerInput'] = {
			name,
			addresses,
			email,
			language,
			currency,
			hourly_rate,
			note,
			vat_id: vatId,
			leitweg_id: leitwegId,
			xrechnung,
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
	<fieldset class="wide">
		<legend>{t('customers.addresses')}</legend>
		{#each addresses as address, i (address)}
			<div class="address">
				<div class="wide row">
					<label class="row">
						<input type="radio" name="default-address" checked={address.is_default} onchange={() => makeDefault(i)} />
						{t('customers.default_address')}
					</label>
					{#if addresses.length > 1}
						<button type="button" class="ghost icon" aria-label={t('app.remove')} onclick={() => removeAddress(i)}>
							<Icon name="close" size={18} />
						</button>
					{/if}
				</div>
				{#if addresses.length > 1}
					<div class="wide">
						<Field label={t('customers.address_label')} error={error?.fields?.[`addresses.${i}.label`]}>
							<input bind:value={address.label} />
						</Field>
					</div>
				{/if}
				<div class="wide">
					<Field
						label={t('customers.street')}
						note={t('customers.street_hint')}
						error={error?.fields?.[`addresses.${i}.street`]}
					>
						<textarea rows="2" bind:value={address.street} autocomplete="street-address"></textarea>
					</Field>
				</div>
				<Field label={t('customers.postcode')} error={error?.fields?.[`addresses.${i}.postcode`]}>
					<input bind:value={address.postcode} autocomplete="postal-code" />
				</Field>
				<Field label={t('customers.city')} error={error?.fields?.[`addresses.${i}.city`]}>
					<input bind:value={address.city} autocomplete="address-level2" />
				</Field>
				<Field label={t('customers.country')} error={error?.fields?.[`addresses.${i}.country`]}>
					<input bind:value={address.country} maxlength="2" autocomplete="country" />
				</Field>
			</div>
		{/each}
		<div>
			<button type="button" class="ghost" onclick={() => addresses.push(blank(false))}>
				<Icon name="plus" size={18} />{t('customers.add_address')}
			</button>
		</div>
	</fieldset>
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
	<Field label={t('term.hourly_rate')} error={error?.fields?.hourly_rate} suffix={currency}>
		<input inputmode="decimal" bind:value={rate} />
	</Field>
	<Field label={t('customers.vat_id')} error={error?.fields?.vat_id}>
		<input bind:value={vatId} />
	</Field>
	<Field label={t('customers.leitweg_id')} error={error?.fields?.leitweg_id}>
		<input bind:value={leitwegId} />
	</Field>
	<div class="wide">
		<Field label={t('customers.xrechnung')} note={t('customers.xrechnung_hint')} inline>
			<input type="checkbox" bind:checked={xrechnung} />
		</Field>
	</div>
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
	fieldset {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		margin: 0;
		padding: 0;
		border: 0;
		min-width: 0;
	}
	legend {
		padding: 0;
		margin-bottom: 0.5rem;
		font-weight: 600;
	}
	.address {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
		gap: 1rem 1.25rem;
		padding: 1rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}
	.address > .row {
		justify-content: space-between;
	}
</style>
