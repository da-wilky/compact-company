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

<div class="page-head">
	<h1>{t('audit.title')}</h1>
</div>

<div class="stack">
	<form class="filter" onsubmit={filter}>
		<input aria-label={t('audit.subject')} placeholder={t('audit.subject')} bind:value={subject} />
		<input aria-label={t('audit.action')} placeholder={t('audit.action')} bind:value={action} />
		<button>{t('audit.filter')}</button>
	</form>

	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	<div class="table-wrap stacked">
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
						<td class="nowrap" data-label={t('audit.at')}>
							{new Date(entry.at * 1000).toLocaleString(i18n.locale)}
						</td>
						<td data-label={t('audit.user')}>{entry.user_name}</td>
						<td data-label={t('audit.action')}><span class="badge">{entry.action}</span></td>
						<td data-label={t('audit.subject')}>{entry.subject}</td>
						<td><code>{change(entry)}</code></td>
					</tr>
				{:else}
					<tr><td colspan="5" class="empty">{t('audit.empty')}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if more}
		<div><button onclick={() => load(entries.at(-1)?.id)}>{t('audit.more')}</button></div>
	{/if}
</div>

<style>
	.filter {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.filter input {
		flex: 1 1 10rem;
		width: auto;
		min-width: 0;
	}
	td {
		vertical-align: top;
	}
	.nowrap {
		white-space: nowrap;
	}
	code {
		word-break: break-word;
	}
</style>
