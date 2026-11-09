<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';

	type Role = Schemas['Role'];
	const roles: Role[] = ['owner', 'staff', 'accountant'];

	let users = $state<Schemas['UserView'][]>([]);
	let invitations = $state<Schemas['InvitationView'][]>([]);
	let name = $state('');
	let role = $state<Role>('staff');
	let created = $state<Schemas['NewInvitation']>();
	let error = $state<ApiError>();

	async function load() {
		const [u, i] = await Promise.all([
			call(api.GET('/api/users')),
			call(api.GET('/api/invitations'))
		]);
		users = u.data ?? [];
		invitations = i.data ?? [];
		error = u.error ?? i.error;
	}

	async function change(id: number, body: Schemas['UserChange']) {
		const result = await call(api.PATCH('/api/users/{id}', { params: { path: { id } }, body }));
		error = result.error;
		await load();
	}

	async function invite(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(api.POST('/api/invitations', { body: { name, role } }));
		error = result.error;
		created = result.data;
		if (result.data) name = '';
		await load();
	}

	async function revoke(id: number) {
		const result = await call(api.DELETE('/api/invitations/{id}', { params: { path: { id } } }));
		error = result.error;
		await load();
	}

	const date = (secs: number) => new Date(secs * 1000).toLocaleDateString(i18n.locale);

	load();
</script>

<h1>{t('users.title')}</h1>

{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}

<table>
	<thead>
		<tr>
			<th>{t('users.name')}</th>
			<th>{t('users.role')}</th>
			<th>{t('users.sign_in')}</th>
			<th>{t('users.active')}</th>
		</tr>
	</thead>
	<tbody>
		{#each users as user (user.id)}
			<tr>
				<td>{user.name}</td>
				<td>
					<select
						value={user.role}
						onchange={(e) => change(user.id, { role: e.currentTarget.value as Role })}
					>
						{#each roles as r (r)}<option value={r}>{t(`term.${r}`)}</option>{/each}
					</select>
				</td>
				<td>
					{t('users.passkeys', { count: user.passkeys })}{#if user.oidc}, OIDC{/if}
				</td>
				<td>
					<input
						type="checkbox"
						checked={user.active}
						onchange={(e) => change(user.id, { active: e.currentTarget.checked })}
					/>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

<h2>{t('invitations.title')}</h2>

{#if created}
	<p role="status">{t('invitations.link', { name: created.invitation.name })}</p>
	<p><code>{created.link}</code></p>
{/if}

<table>
	<tbody>
		{#each invitations as invitation (invitation.id)}
			<tr>
				<td>{invitation.name}</td>
				<td>{t(`term.${invitation.role}`)}</td>
				<td>{t('invitations.expires')} {date(invitation.expires_at)}</td>
				<td><button onclick={() => revoke(invitation.id)}>{t('invitations.revoke')}</button></td>
			</tr>
		{/each}
	</tbody>
</table>

<form onsubmit={invite}>
	<Field label={t('users.name')} error={error?.fields?.name}>
		<input bind:value={name} />
	</Field>
	<Field label={t('users.role')}>
		<select bind:value={role}>
			{#each roles as r (r)}<option value={r}>{t(`term.${r}`)}</option>{/each}
		</select>
	</Field>
	<button>{t('invitations.invite')}</button>
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
	code {
		word-break: break-all;
	}
</style>
