<script lang="ts">
	import Hint from '#lib/components/Hint.svelte';
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { formatDay, isoDay, today } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Year = Schemas['RetentionYear'];
	type Item = Schemas['RetentionItem'];

	let years = $state<Year[]>([]);
	let error = $state<ApiError>();
	let confirmed = $state<Record<string, boolean>>({});

	const day = (iso: string) => formatDay(iso, { dateStyle: 'medium' });
	const at = (seconds: number) => day(isoDay(new Date(seconds * 1000)));

	async function load() {
		const result = await call(api.GET('/api/retention'));
		years = result.data ?? [];
		error = result.error;
	}

	function kind(item: Item) {
		switch (item.kind) {
			case 'receipt':
			case 'bank_statement':
				return t(`retention.kind.${item.kind}`);
			case 'cancellation':
				return t('term.cancellation_invoice');
			case 'dunning':
				return t('term.dunning_letter');
			default:
				return t(`term.${item.kind}`);
		}
	}

	function status(item: Item) {
		if (item.deleted_at) return t('retention.deleted', { date: at(item.deleted_at) });
		if (item.held) return t(`retention.held.${item.held.reason}`, { by: item.held.by });
		return t('retention.from', { date: day(item.deletable_from) });
	}

	function replace(updated: Year) {
		years = years.map((y) => (y.year === updated.year ? updated : y));
	}

	async function remove(year: Year) {
		const result = await call(
			api.POST('/api/retention/{year}/delete', {
				params: { path: { year: year.year } },
				body: { no_open_assessment: !!confirmed[`delete-${year.year}`] }
			})
		);
		error = result.error;
		if (result.data) {
			replace(result.data);
			toast(t('retention.deleted_toast'));
		}
	}

	async function anonymise(year: Year) {
		const result = await call(
			api.POST('/api/retention/{year}/anonymise', {
				params: { path: { year: year.year } },
				body: { no_open_assessment: !!confirmed[`anonymise-${year.year}`] }
			})
		);
		error = result.error;
		if (result.data) {
			replace(result.data);
			toast(t('retention.anonymised_toast'));
		}
	}

	const due = (year: Year) =>
		!year.anonymisation.anonymised_at && !year.anonymisation.blockers.length && year.anonymisation.from <= today();

	load();
</script>

<div class="page-head">
	<div>
		<h1>{t('retention.title')}<Hint term="retention" /></h1>
		<p class="muted">{t('retention.intro')}</p>
	</div>
</div>

{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}

<div class="stack">
	{#each years as year (year.year)}
		<details class="card" open={year.deletable > 0 || due(year)}>
			<summary>
				<strong>{year.year}</strong>
				{#if year.anonymisation.anonymised_at}
					<span class="badge">{t('retention.anonymised')}</span>
				{:else}
					{#if year.deletable}<span class="badge accent">{t('retention.deletable', { count: year.deletable })}</span>{/if}
					{#if year.pruned}<span class="badge">{t('retention.pruned')}</span>{/if}
				{/if}
			</summary>
			<div class="stack body">
				{#if year.items.length}
					<div class="table-wrap stacked">
						<table>
							<thead>
								<tr>
									<th>{t('term.document')}</th>
									<th>{t('books.date')}</th>
									<th>{t('term.retention_period')}</th>
									<th>{t('term.status')}</th>
								</tr>
							</thead>
							<tbody>
								{#each year.items as item (item.subject)}
									<tr class:gone={item.deleted_at}>
										<td data-label={t('term.document')}>
											<span class="muted">{kind(item)}</span>
											{item.label}
										</td>
										<td data-label={t('books.date')}>{day(item.date)}</td>
										<td data-label={t('term.retention_period')}>{t(`retention.class.${item.class}`)}</td>
										<td data-label={t('term.status')} class:held={item.held}>{status(item)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{:else}
					<p class="muted">{t('retention.nothing')}</p>
				{/if}

				{#each year.deletions as deletion (deletion.at)}
					<p class="muted">{t('retention.deletion', { date: at(deletion.at), count: deletion.files, name: deletion.user })}</p>
				{/each}

				{#if year.deletable && !year.anonymisation.anonymised_at}
					<div class="confirm stack">
						<label class="check">
							<input type="checkbox" bind:checked={confirmed[`delete-${year.year}`]} />
							{t('retention.confirm', { year: year.year })}
						</label>
						<div>
							<button class="danger" disabled={!confirmed[`delete-${year.year}`]} onclick={() => remove(year)}>
								{t('retention.delete', { count: year.deletable })}
							</button>
						</div>
					</div>
				{/if}

				<h3>{t('retention.anonymise_title')}</h3>
				{#if year.anonymisation.anonymised_at}
					<p>{t('retention.anonymised_at', { date: at(year.anonymisation.anonymised_at) })}</p>
				{:else}
					<p class="muted">{t('retention.anonymise_from', { date: day(year.anonymisation.from) })}</p>
					{#if year.anonymisation.from <= today()}
						{#each year.anonymisation.blockers as blocker, i (i)}
							<p class="held">{t(`retention.blocked.${blocker.reason}`, { by: String(blocker.by) })}</p>
						{/each}
					{/if}
					{#if due(year)}
						{@const names = [...year.anonymisation.customers, ...year.anonymisation.suppliers, ...year.anonymisation.users]}
						{#if names.length}
							<p>{t('retention.names')}</p>
							<ul class="names">
								{#each names as named, i (i)}<li>{named.name}</li>{/each}
							</ul>
						{:else}
							<p class="muted">{t('retention.no_names')}</p>
						{/if}
						<div class="confirm stack">
							<label class="check">
								<input type="checkbox" bind:checked={confirmed[`anonymise-${year.year}`]} />
								{t('retention.confirm', { year: year.year })}
							</label>
							<div>
								<button class="danger" disabled={!confirmed[`anonymise-${year.year}`]} onclick={() => anonymise(year)}>
									{t('retention.anonymise', { year: year.year })}
								</button>
							</div>
						</div>
					{/if}
				{/if}
			</div>
		</details>
	{:else}
		<p class="card empty">{t('tax.archive_hint')}</p>
	{/each}
</div>

<style>
	summary {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		cursor: pointer;
	}
	.body {
		margin-top: 1rem;
	}
	.gone td {
		color: var(--muted);
	}
	.held {
		color: var(--accent-text);
	}
	.check {
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
	}
	.check input {
		width: auto;
		margin-top: 0.2rem;
	}
	.names {
		margin: 0;
		padding-left: 1.25rem;
	}
	h3 {
		margin: 0.5rem 0 0;
	}
</style>
