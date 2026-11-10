<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Role = Schemas['Role'];
	const roles: Role[] = ['owner', 'staff', 'accountant'];

	let users = $state<Schemas['UserView'][]>([]);
	let invitations = $state<Schemas['InvitationView'][]>([]);
	let name = $state('');
	let role = $state<Role>('staff');
	let created = $state<Schemas['NewInvitation']>();
	let showLink = $state(false);
	let revoking = $state<Schemas['InvitationView']>();
	let confirming = $state(false);
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
		if (result.data) {
			name = '';
			showLink = true;
		}
		await load();
	}

	function ask(invitation: Schemas['InvitationView']) {
		revoking = invitation;
		confirming = true;
	}

	async function revoke() {
		if (!revoking) return;
		const id = revoking.id;
		confirming = false;
		const result = await call(api.DELETE('/api/invitations/{id}', { params: { path: { id } } }));
		error = result.error;
		await load();
	}

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast(t('app.copied'));
		} catch {
			// without clipboard access the link stays selectable in the dialog
		}
	}

	const date = (secs: number) => new Date(secs * 1000).toLocaleDateString(i18n.locale);

	load();
</script>

<div class="page-head">
	<h1>{t('users.title')}</h1>
</div>

<div class="stack">
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	<div class="table-wrap stacked">
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
					<tr class:inactive={!user.active}>
						<td data-label={t('users.name')}><strong>{user.name}</strong></td>
						<td data-label={t('users.role')}>
							<select
								aria-label={t('users.role')}
								value={user.role}
								onchange={(e) => change(user.id, { role: e.currentTarget.value as Role })}
							>
								{#each roles as r (r)}<option value={r}>{t(`term.${r}`)}</option>{/each}
							</select>
						</td>
						<td data-label={t('users.sign_in')}>
							<span class="row">
								<span class="badge">{t('users.passkeys', { count: user.passkeys })}</span>
								{#if user.oidc}<span class="badge">OIDC</span>{/if}
							</span>
						</td>
						<td data-label={t('users.active')}>
							<input
								type="checkbox"
								aria-label={t('users.active')}
								checked={user.active}
								onchange={(e) => change(user.id, { active: e.currentTarget.checked })}
							/>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>

	<section class="card stack">
		<h2>{t('invitations.title')}</h2>
		{#if invitations.length}
			<ul class="list">
				{#each invitations as invitation (invitation.id)}
					<li>
						<div>
							<strong>{invitation.name}</strong>
							<small class="muted">
								{t(`term.${invitation.role}`)} · {t('invitations.expires')}
								{date(invitation.expires_at)}
							</small>
						</div>
						<button class="ghost danger" onclick={() => ask(invitation)}>
							{t('invitations.revoke')}
						</button>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="muted">{t('invitations.none')}</p>
		{/if}
		<form class="invite" onsubmit={invite}>
			<Field label={t('users.name')} error={error?.fields?.name}>
				<input bind:value={name} />
			</Field>
			<Field label={t('users.role')}>
				<select bind:value={role}>
					{#each roles as r (r)}<option value={r}>{t(`term.${r}`)}</option>{/each}
				</select>
			</Field>
			<button class="primary">{t('invitations.invite')}</button>
		</form>
	</section>
</div>

<Dialog bind:open={showLink} title={t('invitations.created')}>
	{#if created}
		<div class="stack">
			<p>{t('invitations.link', { name: created.invitation.name })}</p>
			<code class="link">{created.link}</code>
		</div>
	{/if}
	{#snippet actions()}
		<button onclick={() => (showLink = false)}>{t('app.close')}</button>
		<button class="primary" onclick={() => created && copy(created.link)}>
			<Icon name="copy" size={18} />{t('app.copy')}
		</button>
	{/snippet}
</Dialog>

<Dialog bind:open={confirming} title={t('invitations.revoke')}>
	<p>{t('invitations.revoke_confirm', { name: revoking?.name ?? '' })}</p>
	{#snippet actions()}
		<button onclick={() => (confirming = false)}>{t('app.cancel')}</button>
		<button class="primary" onclick={revoke}>{t('invitations.revoke')}</button>
	{/snippet}
</Dialog>

<style>
	select {
		width: auto;
	}
	.inactive td {
		color: var(--muted);
	}
	.invite {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-end;
		gap: 0.75rem;
	}
	.invite > :global(label) {
		flex: 1 1 12rem;
	}
	.link {
		display: block;
		padding: 0.75rem;
		border-radius: var(--radius);
		background: var(--surface-2);
		word-break: break-all;
		user-select: all;
	}
</style>
