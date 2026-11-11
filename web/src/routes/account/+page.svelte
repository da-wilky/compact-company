<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Preferences from '#lib/components/Preferences.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';
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

	const scopes: Schemas['Scope'][] = ['work:read', 'work:write', 'documents:read', 'documents:write', 'books:read'];
	let tokens = $state<Schemas['Token'][]>([]);
	let tokenName = $state('');
	let chosen = $state<Schemas['Scope'][]>(['work:read']);
	let secret = $state<string>();
	let tokenError = $state<ApiError>();

	call(api.GET('/api/tokens')).then(({ data }) => (tokens = data ?? []));

	async function createToken(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(api.POST('/api/tokens', { body: { name: tokenName, scopes: chosen } }));
		tokenError = result.error;
		if (result.data) {
			secret = result.data.secret ?? undefined;
			tokens.push(result.data);
			tokenName = '';
		}
	}

	async function revoke(token: Schemas['Token']) {
		if (!confirm(t('tokens.revoke_confirm', { name: token.name }))) return;
		const result = await call(api.DELETE('/api/tokens/{id}', { params: { path: { id: token.id } } }));
		if (result.data) tokens = result.data;
		tokenError = result.error;
	}

	async function copy(value: string) {
		await navigator.clipboard.writeText(value);
		toast(t('app.copied'));
	}
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
		<h2>{t('tokens.title')}</h2>
		<p class="muted">{t('tokens.hint')}</p>
		{#if tokens.length}
			<ul class="list">
				{#each tokens as token (token.id)}
					<li>
						<Icon name="key" />
						<div>
							<strong>{token.name}</strong>
							<small class="muted">
								{token.scopes.join(' · ')} · {t('passkeys.last_used')}
								{date(token.last_used_at)}
							</small>
						</div>
						<button class="ghost danger" onclick={() => revoke(token)}>{t('tokens.revoke')}</button>
					</li>
				{/each}
			</ul>
		{/if}
		{#if secret}
			<div class="secret">
				<input readonly value={secret} aria-label={t('tokens.title')} />
				<button type="button" class="ghost icon" aria-label={t('app.copy')} onclick={() => copy(secret!)}>
					<Icon name="copy" size={18} />
				</button>
			</div>
			<small class="muted">{t('tokens.once')}</small>
		{/if}
		<form class="stack" onsubmit={createToken}>
			<Field label={t('tokens.name')} error={tokenError?.fields?.name}>
				<input bind:value={tokenName} placeholder={t('tokens.name_hint')} />
			</Field>
			<fieldset class="scopes">
				<legend>{t('tokens.scopes')}</legend>
				{#each scopes as scope (scope)}
					<label>
						<input type="checkbox" bind:group={chosen} value={scope} />
						<span><code>{scope}</code> {t(`tokens.scope.${scope}`)}</span>
					</label>
				{/each}
				{#if tokenError?.fields?.scopes}<small class="error">{t('tokens.choose_scope')}</small>{/if}
			</fieldset>
			<div><button class="primary">{t('tokens.create')}</button></div>
		</form>
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
	.secret {
		display: flex;
		gap: 0.5rem;
	}
	.secret input {
		flex: 1;
		min-width: 0;
		font-family: monospace;
	}
	.scopes {
		display: grid;
		gap: 0.5rem;
		margin: 0;
		padding: 0;
		border: 0;
	}
	.scopes legend {
		margin-bottom: 0.375rem;
		font-weight: 500;
	}
	.scopes label {
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
	}
	.scopes input {
		width: auto;
		margin-top: 0.2rem;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
</style>
