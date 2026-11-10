<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { startOidc } from '#lib/oidc.ts';
	import { session, show } from '#lib/session.svelte.ts';
	import { createPasskey } from '#lib/webauthn.ts';

	const token = location.hash.slice(1);
	let info = $state<Schemas['InvitationInfo']>();
	let passkeyName = $state('');
	let error = $state<ApiError>();

	call(api.POST('/api/invitation', { body: { token } })).then((result) => {
		info = result.data;
		error = result.error;
	});

	async function accept(event: SubmitEvent) {
		event.preventDefault();
		const start = await call(api.POST('/api/invitation/passkey/start', { body: { token } }));
		error = start.error;
		if (!start.data) return;
		const passkey = await createPasskey(start.data.options);
		error = passkey.error;
		if (passkey.error) return;
		const finish = await call(
			api.POST('/api/invitation/passkey/finish', {
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

<h1>{t('invitation.title')}</h1>

{#if info}
	<p>{t('invitation.intro', { name: info.name, role: t(`term.${info.role}`) })}</p>
	<form onsubmit={accept}>
		<Field
			label={t('passkeys.name')}
			note={t('passkeys.name_hint')}
			error={error?.fields?.passkey_name}
		>
			<input bind:value={passkeyName} />
		</Field>
		<button>{t('passkeys.create')}</button>
		{#if session.view?.oidc}
			<button type="button" onclick={async () => (error = await startOidc(token))}>
				{t('invitation.oidc', { name: session.view.oidc })}
			</button>
		{/if}
	</form>
{/if}
{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
