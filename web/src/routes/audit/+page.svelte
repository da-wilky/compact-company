<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';

	let entries = $state<Schemas['AuditEntry'][]>([]);
	let subject = $state('');
	let action = $state('');
	let more = $state(false);
	let error = $state<ApiError>();

	const limit = 100;

	async function load(before_id?: number) {
		const query = { subject: subject || undefined, action: action || undefined, before_id, limit };
		const result = await call(api.GET('/api/audit', { params: { query } }));
		error = result.error;
		const page = result.data ?? [];
		entries = before_id ? [...entries, ...page] : page;
		more = page.length === limit;
	}

	function filter(event: SubmitEvent) {
		event.preventDefault();
		load();
	}

	const change = (entry: Schemas['AuditEntry']) =>
		[entry.before, entry.after].map((v) => (v == null ? '—' : JSON.stringify(v))).join(' → ');

	load();
</script>

<h1>{t('audit.title')}</h1>

<form onsubmit={filter}>
	<input placeholder={t('audit.subject')} bind:value={subject} />
	<input placeholder={t('audit.action')} bind:value={action} />
	<button>{t('audit.filter')}</button>
</form>

{#if error}<p class="error" role="alert">{t(error.message)}</p>{/if}

<div class="scroll">
	<table>
		<thead>
			<tr>
				<th>{t('audit.at')}</th>
				<th>{t('audit.user')}</th>
				<th>{t('audit.action')}</th>
				<th>{t('audit.subject')}</th>
				<th>{t('audit.change')}</th>
			</tr>
		</thead>
		<tbody>
			{#each entries as entry (entry.id)}
				<tr>
					<td>{new Date(entry.at * 1000).toLocaleString(i18n.locale)}</td>
					<td>{entry.user_name}</td>
					<td>{entry.action}</td>
					<td>{entry.subject}</td>
					<td><code>{change(entry)}</code></td>
				</tr>
			{:else}
				<tr><td colspan="5">{t('audit.empty')}</td></tr>
			{/each}
		</tbody>
	</table>
</div>

{#if more}
	<button onclick={() => load(entries.at(-1)?.id)}>{t('audit.more')}</button>
{/if}

<style>
	.scroll {
		overflow-x: auto;
	}
	form input {
		flex: 1 1 10rem;
		min-width: 0;
	}
	form {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}
	table {
		border-collapse: collapse;
		width: 100%;
		margin-bottom: 1rem;
	}
	th,
	td {
		text-align: left;
		padding: 0.4rem;
		border-bottom: 1px solid #ddd;
		vertical-align: top;
	}
	code {
		word-break: break-word;
	}
</style>
