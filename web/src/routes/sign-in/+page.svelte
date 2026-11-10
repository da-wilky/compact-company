<script lang="ts">
	import { api, call, clientError, type ApiError } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { has, t } from '#lib/i18n.svelte.ts';
	import { startOidc } from '#lib/oidc.ts';
	import { session, show } from '#lib/session.svelte.ts';
	import { usePasskey } from '#lib/webauthn.ts';

	// the OIDC callback reports its failure in the fragment; anything else there is not ours to show
	const reported = decodeURIComponent(location.hash.slice(1));
	let error = $state<ApiError | undefined>(
		reported.startsWith('error.') && has(reported) ? clientError(reported) : undefined
	);

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
<div class="stack">
	<button class="primary" onclick={signIn}><Icon name="passkey" />{t('sign_in.submit')}</button>
	{#if session.view?.oidc}
		<button onclick={async () => (error = await startOidc())}>
			{t('sign_in.oidc', { name: session.view.oidc })}
		</button>
	{/if}
</div>
{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
