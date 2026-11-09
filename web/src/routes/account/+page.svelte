<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { createPasskey } from '#lib/webauthn.ts';

	let passkeys = $state<Schemas['PasskeyView'][]>([]);
	let name = $state('');
	let error = $state<ApiError>();

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

	async function remove(id: number) {
		const result = await call(api.DELETE('/api/passkeys/{id}', { params: { path: { id } } }));
		if (result.data) passkeys = result.data;
		error = result.error;
	}

	const date = (secs: number | null | undefined) =>
		secs ? new Date(secs * 1000).toLocaleString(i18n.locale) : t('app.never');

	load();
</script>

<h1>{t('account.title')}</h1>

<h2>{t('passkeys.title')}</h2>
<table>
	<thead>
		<tr>
			<th>{t('passkeys.name')}</th>
			<th>{t('passkeys.created')}</th>
			<th>{t('passkeys.last_used')}</th>
			<th></th>
		</tr>
	</thead>
	<tbody>
		{#each passkeys as passkey (passkey.id)}
			<tr>
				<td>{passkey.name}</td>
				<td>{date(passkey.created_at)}</td>
				<td>{date(passkey.last_used_at)}</td>
				<td>
					<button onclick={() => remove(passkey.id)} disabled={passkeys.length === 1}>
						{t('app.remove')}
					</button>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

<form onsubmit={add}>
	<Field label={t('passkeys.name')} note={t('passkeys.name_hint')} error={error?.fields?.passkey_name}>
		<input bind:value={name} />
	</Field>
	{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}
	<button>{t('passkeys.add')}</button>
</form>

<style>
	table {
		border-collapse: collapse;
		width: 100%;
		margin-bottom: 1.5rem;
	}
	th,
	td {
		text-align: left;
		padding: 0.4rem;
		border-bottom: 1px solid #ddd;
	}
</style>
