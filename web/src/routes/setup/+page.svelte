<script lang="ts">
	import { api, call, type ApiError } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { show } from '#lib/session.svelte.ts';
	import { createPasskey } from '#lib/webauthn.ts';

	const token = location.hash.slice(1);
	let name = $state('');
	let passkeyName = $state('');
	let error = $state<ApiError>();

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		const start = await call(api.POST('/api/setup/start', { body: { token, name } }));
		error = start.error;
		if (!start.data) return;
		const passkey = await createPasskey(start.data.options);
		error = passkey.error;
		if (passkey.error) return;
		const finish = await call(
			api.POST('/api/setup/finish', {
				body: {
					ceremony: start.data.ceremony,
					passkey_name: passkeyName,
					credential: passkey.credential as { [key: string]: unknown }
				}
			})
		);
		error = finish.error;
		if (finish.data) show(finish.data);
	}
</script>

<h1>{t('setup.title')}</h1>

{#if token}
	<p>{t('setup.intro')}</p>
	<form onsubmit={submit}>
		<Field label={t('setup.name')} error={error?.fields?.name}>
			<input bind:value={name} autocomplete="name" />
		</Field>
		<Field
			label={t('passkeys.name')}
			note={t('passkeys.name_hint')}
			error={error?.fields?.passkey_name}
		>
			<input bind:value={passkeyName} />
		</Field>
		{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
		<button>{t('passkeys.create')}</button>
	</form>
{:else}
	<p>{t('setup.no_token')}</p>
{/if}
