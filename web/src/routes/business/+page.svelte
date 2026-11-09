<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';

	let values = $state<Schemas['Business']>({});
	let pinned = $state<string[]>([]);
	let error = $state<ApiError>();
	let saved = $state(false);

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
		saved = !result.error;
	}

	call(api.GET('/api/business')).then(show);

	const note = (key: string) => (pinned.includes(key) ? t('business.pinned') : undefined);
	const texts = ['name', 'tax_number', 'vat_id'] as const;
</script>

<h1>{t('business.title')}</h1>

<form onsubmit={save}>
	{#each texts as key (key)}
		<Field label={t(`business.${key}`)} error={error?.fields?.[key]} note={note(key)}>
			<input bind:value={values[key]} readonly={pinned.includes(key)} />
		</Field>
	{/each}
	<Field label={t('business.address')} error={error?.fields?.address} note={note('address')}>
		<textarea rows="3" bind:value={values.address} readonly={pinned.includes('address')}
		></textarea>
	</Field>
	<Field label={t('business.small_business')} note={note('small_business')}>
		<input
			type="checkbox"
			bind:checked={values.small_business}
			disabled={pinned.includes('small_business')}
		/>
	</Field>

	{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
	{#if saved}<p role="status">{t('app.saved')}</p>{/if}
	<button>{t('app.save')}</button>
</form>
