<script lang="ts">
	import { api, call, type ApiError } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	let rows = $state<{ key: number; id?: number; name: string; done: boolean }[]>([]);
	let error = $state<ApiError>();
	let next = 0;

	call(api.GET('/api/statuses')).then((result) => {
		rows = (result.data ?? []).map((s) => ({ key: next++, ...s }));
		error = result.error;
	});

	function shift(i: number, by: number) {
		const [row] = rows.splice(i, 1);
		rows.splice(i + by, 0, row);
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(
			api.PUT('/api/statuses', { body: rows.map(({ id, name, done }) => ({ id, name, done })) })
		);
		error = result.error;
		if (result.data) {
			rows = result.data.map((s) => ({ key: next++, ...s }));
			toast(t('app.saved'));
		}
	}
</script>

<div class="page-head">
	<div>
		<h1>{t('statuses.title')}</h1>
		<p class="muted">{t('statuses.intro')}</p>
	</div>
</div>

<form class="card stack" onsubmit={save}>
	<ol class="list">
		{#each rows as row, i (row.key)}
			<li>
				<span class="position">{i + 1}</span>
				<input aria-label={t('statuses.name')} bind:value={row.name} class:invalid={error?.fields?.[`${i}.name`]} />
				<label class="row done"><input type="checkbox" bind:checked={row.done} />{t('statuses.done')}</label>
				<button type="button" class="ghost icon" aria-label={t('statuses.up')} disabled={i === 0} onclick={() => shift(i, -1)}>
					<Icon name="chevron_up" size={18} />
				</button>
				<button type="button" class="ghost icon" aria-label={t('statuses.down')} disabled={i === rows.length - 1} onclick={() => shift(i, 1)}>
					<Icon name="chevron_down" size={18} />
				</button>
				<button type="button" class="ghost icon" aria-label={t('app.remove')} disabled={rows.length === 1} onclick={() => rows.splice(i, 1)}>
					<Icon name="close" size={18} />
				</button>
			</li>
		{/each}
	</ol>
	<div class="row">
		<button type="button" onclick={() => rows.push({ key: next++, name: '', done: false })}>
			<Icon name="plus" size={18} />{t('statuses.add')}
		</button>
		<button class="primary">{t('app.save')}</button>
	</div>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
</form>

<style>
	form {
		max-width: 40rem;
	}
	li {
		flex-wrap: wrap;
	}
	.position {
		width: 1.25rem;
		color: var(--muted);
		font-variant-numeric: tabular-nums;
	}
	li > input {
		flex: 1 1 10rem;
		width: auto;
	}
	.invalid {
		border-color: var(--danger);
	}
	.done {
		gap: 0.375rem;
		color: var(--muted);
	}
</style>
