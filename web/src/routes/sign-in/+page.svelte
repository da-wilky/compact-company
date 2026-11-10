<script lang="ts">
	import { api, call, type ApiError } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { show } from '#lib/session.svelte.ts';
	import { usePasskey } from '#lib/webauthn.ts';

	let error = $state<ApiError>();

	async function signIn() {
		const start = await call(api.POST('/api/sign-in/start'));
		error = start.error;
		if (!start.data) return;
		const passkey = await usePasskey(start.data.options);
		error = passkey.error;
		if (passkey.error) return;
		const finish = await call(
			api.POST('/api/sign-in/finish', {
				body: {
					ceremony: start.data.ceremony,
					credential: passkey.credential as { [key: string]: unknown }
				}
			})
		);
		error = finish.error;
		if (finish.data) show(finish.data);
	}
</script>

<h1>{t('sign_in.title')}</h1>
<button onclick={signIn}>{t('sign_in.submit')}</button>
{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
