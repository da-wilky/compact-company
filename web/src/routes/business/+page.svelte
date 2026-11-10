<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	const owner = $derived(session.view?.user?.role === 'owner');

	let values = $state<Schemas['Business']>({});
	let pinned = $state<string[]>([]);
	let error = $state<ApiError>();

	function show(result: { data?: Schemas['BusinessView']; error?: ApiError }) {
		if (result.data) {
			values = result.data.values;
			pinned = result.data.pinned;
		}
		error = result.error;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(api.PUT('/api/business', { body: values }));
		show(result);
		if (!result.error) toast(t('app.saved'));
	}

	call(api.GET('/api/business')).then(show);
	let currencies = $state<string[]>([]);
	call(api.GET('/api/currencies')).then(({ data }) => (currencies = data ?? []));

	const note = (key: string) => (pinned.includes(key) ? t('business.pinned') : undefined);
	const texts = ['name', 'tax_number', 'vat_id'] as const;
</script>

<div class="page-head">
	<h1>{t('business.title')}</h1>
</div>

<form class="card" onsubmit={save}>
	<fieldset disabled={!owner}>
		{#each texts as key (key)}
			<Field label={t(`business.${key}`)} error={error?.fields?.[key]} note={note(key)}>
				<input bind:value={values[key]} readonly={pinned.includes(key)} />
			</Field>
		{/each}
		<Field label={t('business.address')} error={error?.fields?.address} note={note('address')}>
			<textarea rows="3" bind:value={values.address} readonly={pinned.includes('address')}
			></textarea>
		</Field>
		<Field label={t('business.currency')} error={error?.fields?.currency} note={note('currency')}>
			<select bind:value={values.currency} disabled={pinned.includes('currency')}>
				{#each currencies as code (code)}<option value={code}>{code}</option>{/each}
			</select>
		</Field>
		<div class="wide">
			<Field label={t('business.small_business')} note={note('small_business')} inline>
				<input
					type="checkbox"
					bind:checked={values.small_business}
					disabled={pinned.includes('small_business')}
				/>
			</Field>
		</div>

		{#if error}<p class="alert wide" role="alert">{t(error.message)}</p>{/if}
		{#if owner}<div class="wide"><button class="primary">{t('app.save')}</button></div>{/if}
	</fieldset>
</form>

<style>
	form {
		max-width: 48rem;
	}
	fieldset {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
		gap: 1.25rem;
		border: 0;
		padding: 0;
		margin: 0;
	}
	.wide {
		grid-column: 1 / -1;
	}
</style>
