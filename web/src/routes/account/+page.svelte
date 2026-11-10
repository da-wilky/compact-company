<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Preferences from '#lib/components/Preferences.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { createPasskey } from '#lib/webauthn.ts';

	let passkeys = $state<Schemas['PasskeyView'][]>([]);
	let name = $state('');
	let error = $state<ApiError>();
	let removing = $state<Schemas['PasskeyView']>();
	let confirming = $state(false);

	async function load() {
		const result = await call(api.GET('/api/passkeys'));
		if (result.data) passkeys = result.data;
		error = result.error;
	}

	async function add(event: SubmitEvent) {
		event.preventDefault();
		const start = await call(api.POST('/api/passkeys/start'));
		error = start.error;
		if (!start.data) return;
		const passkey = await createPasskey(start.data.options);
		error = passkey.error;
		if (passkey.error) return;
		const finish = await call(
			api.POST('/api/passkeys/finish', {
				body: {
					ceremony: start.data.ceremony,
					passkey_name: name,
					credential: passkey.credential as { [key: string]: unknown }
				}
			})
		);
		error = finish.error;
		if (finish.data) {
			passkeys = finish.data;
			name = '';
		}
	}

	function ask(passkey: Schemas['PasskeyView']) {
		removing = passkey;
		confirming = true;
	}

	async function remove() {
		if (!removing) return;
		const id = removing.id;
		confirming = false;
		const result = await call(api.DELETE('/api/passkeys/{id}', { params: { path: { id } } }));
		if (result.data) passkeys = result.data;
		error = result.error;
	}

	const date = (secs: number | null | undefined) =>
		secs ? new Date(secs * 1000).toLocaleString(i18n.locale) : t('app.never');

	load();
</script>

<div class="page-head">
	<div>
		<p class="muted">{t('account.title')}</p>
		<h1>{session.view?.user?.name}</h1>
	</div>
	{#if session.view?.user}<span class="badge accent">{t(`term.${session.view.user.role}`)}</span>{/if}
</div>

<div class="stack">
	<section class="card stack">
		<h2>{t('passkeys.title')}</h2>
		<ul class="list">
			{#each passkeys as passkey (passkey.id)}
				<li>
					<Icon name="passkey" />
					<div>
						<strong>{passkey.name}</strong>
						<small class="muted">
							{t('passkeys.created')}
							{date(passkey.created_at)} · {t('passkeys.last_used')}
							{date(passkey.last_used_at)}
						</small>
					</div>
					<button
						class="ghost danger"
						onclick={() => ask(passkey)}
						disabled={passkeys.length === 1}
						title={passkeys.length === 1 ? t('error.last_passkey') : undefined}
					>
						{t('app.remove')}
					</button>
				</li>
			{/each}
		</ul>
		<form class="stack" onsubmit={add}>
			<Field
				label={t('passkeys.name')}
				note={t('passkeys.name_hint')}
				error={error?.fields?.passkey_name}
			>
				<input bind:value={name} />
			</Field>
			<div><button class="primary">{t('passkeys.add')}</button></div>
		</form>
		{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	</section>

	<section class="card stack">
		<h2>{t('appearance.title')}</h2>
		<Preferences />
	</section>
</div>

<Dialog bind:open={confirming} title={t('passkeys.remove_title')}>
	<p>{t('passkeys.remove_confirm', { name: removing?.name ?? '' })}</p>
	{#snippet actions()}
		<button onclick={() => (confirming = false)}>{t('app.cancel')}</button>
		<button class="primary" onclick={remove}>{t('app.remove')}</button>
	{/snippet}
</Dialog>

<style>
	section {
		max-width: 48rem;
	}
	li > :global(svg) {
		flex: none;
		color: var(--muted);
	}
</style>
