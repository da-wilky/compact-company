<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { confirm } from '#lib/confirm.svelte.ts';
	import { i18n, t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	let backup = $state<Schemas['BackupStatus']>();
	let mirror = $state<Schemas['MirrorStatus']>();
	let error = $state<ApiError>();

	const at = (seconds: number) =>
		new Date(seconds * 1000).toLocaleString(i18n.locale, { dateStyle: 'medium', timeStyle: 'short' });

	async function load() {
		const [b, m] = await Promise.all([call(api.GET('/api/backup')), call(api.GET('/api/mirror'))]);
		backup = b.data;
		mirror = m.data;
		error = b.error ?? m.error;
	}

	async function run() {
		const result = await call(api.POST('/api/backup/run'));
		error = result.error;
		if (result.error) return;
		toast(t('backup.started'));
		await load();
		setTimeout(load, 3000);
	}

	async function resync() {
		if (!(await confirm({ text: t('mirror.resync_confirm'), action: t('mirror.resync') }))) return;
		const result = await call(api.POST('/api/mirror/resync'));
		error = result.error;
		if (result.error) return;
		toast(t('mirror.resync_started'));
		await load();
	}

	load();
</script>

<div class="page-head">
	<div>
		<h1>{t('backup.title')}</h1>
		<p class="muted">{t('backup.intro')}</p>
	</div>
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

<div class="stack">
	{#if backup}
		<section class="card stack">
			<h2>{t('term.backup')}</h2>
			{#if backup.configured}
				<p>{t('backup.schedule', { time: backup.time ?? '', daily: backup.keep_daily ?? 0, monthly: backup.keep_monthly ?? 0 })}</p>
				{#if backup.failure}
					<p class="alert" role="alert">
						{t('backup.failed', { date: at(backup.failure.started_at), error: backup.failure.error ?? '' })}
					</p>
				{/if}
				{#if backup.last_success?.finished_at}
					<p>
						{t('backup.last_success', { date: at(backup.last_success.finished_at), files: backup.last_success.uploaded ?? 0 })}
						<br /><code>{backup.last_success.snapshot}</code>
					</p>
				{:else}
					<p class="muted">{t('backup.never')}</p>
				{/if}
				<p class="muted">{t('backup.restore_hint')}</p>
				<pre><code>compact-company restore --config … --identity … --snapshot …</code></pre>
				<div>
					{#if backup.running}
						<p class="muted" role="status">{t('backup.running')}</p>
					{:else}
						<button class="primary" onclick={run}><Icon name="archive" size={18} />{t('backup.run')}</button>
					{/if}
				</div>
			{:else}
				<p class="muted">{t('error.backup_not_configured')}</p>
			{/if}
		</section>
	{/if}

	{#if mirror}
		<section class="card stack">
			<h2>{t('term.mirror')}</h2>
			<p class="muted">{t('mirror.intro')}</p>
			{#if mirror.configured}
				<p>
					{#if mirror.pending}{t('mirror.pending', { count: mirror.pending })}{:else}{t('mirror.up_to_date')}{/if}
					{#if mirror.last_copy_at}<br /><span class="muted">{t('mirror.last_copy', { date: at(mirror.last_copy_at) })}</span>{/if}
				</p>
				{#if mirror.error && mirror.failed_at}
					<p class="alert" role="alert">{t('mirror.error', { date: at(mirror.failed_at), error: mirror.error })}</p>
				{/if}
				<div><button onclick={resync}>{t('mirror.resync')}</button></div>
			{:else}
				<p class="muted">{t('error.mirror_not_configured')}</p>
			{/if}
		</section>
	{/if}
</div>

<style>
	code {
		word-break: break-all;
	}
</style>
