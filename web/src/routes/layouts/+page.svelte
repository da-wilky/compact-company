<script lang="ts">
	import { api, call, upload, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { kindLabel, kinds, type Kind } from '#lib/documents.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Layout = Schemas['LayoutView'];
	type Choice = Schemas['LayoutChoice'];

	let layouts = $state<Layout[]>([]);
	let choices = $state<Choice[]>([]);
	let packages = $state<Schemas['Package'][]>([]);
	let customers = $state<Schemas['Customer'][]>([]);
	let error = $state<ApiError>();

	let open = $state(false);
	let editing = $state<Layout>();
	let name = $state('');
	let chosenKinds = $state<Kind[]>([]);
	let source = $state<FileList>();
	let files = $state<FileList>();
	let uploadError = $state<ApiError>();
	let busy = $state(false);

	let spec = $state('');
	let packageError = $state<ApiError>();

	async function load() {
		const [l, c, p, cu] = await Promise.all([
			call(api.GET('/api/layouts')),
			call(api.GET('/api/layout-choices')),
			call(api.GET('/api/packages')),
			call(api.GET('/api/customers'))
		]);
		layouts = l.data ?? [];
		choices = c.data ?? [];
		packages = p.data ?? [];
		customers = cu.data ?? [];
		error = l.error ?? c.error ?? p.error;
	}
	load();

	function edit(layout?: Layout) {
		editing = layout;
		name = layout?.name ?? '';
		chosenKinds = layout ? [...layout.kinds] : ['invoice', 'quote'];
		source = undefined;
		files = undefined;
		uploadError = undefined;
		open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const form = new FormData();
		form.append('name', name);
		for (const kind of chosenKinds) form.append('kinds', kind);
		if (source?.[0]) form.append('source', source[0], 'layout.typ');
		for (const file of files ?? []) form.append('files', file, file.name);
		busy = true;
		const result = editing
			? await upload<Layout>('PUT', `/api/layouts/${editing.id}`, form)
			: await upload<Layout>('POST', '/api/layouts', form);
		busy = false;
		uploadError = result.error;
		if (result.data) {
			open = false;
			toast(t('app.saved'));
			await load();
		}
	}

	async function remove(layout: Layout) {
		if (!confirm(t('layouts.delete_confirm', { name: layout.name }))) return;
		const result = await call(
			api.DELETE('/api/layouts/{id}', { params: { path: { id: layout.id } } })
		);
		error = result.error;
		await load();
	}

	const fitting = (kind: Kind) => layouts.filter((l) => l.kinds.includes(kind));
	const defaultOf = (kind: Kind) =>
		choices.find((c) => c.kind === kind && c.customer_id == null)?.layout_id ?? 0;
	const overrides = $derived(choices.filter((c) => c.customer_id != null));

	function setDefault(kind: Kind, layout_id: number) {
		choices = choices.filter((c) => !(c.kind === kind && c.customer_id == null));
		if (layout_id) choices.push({ kind, layout_id });
	}

	function addOverride() {
		const layout = layouts[0];
		const customer = customers[0];
		if (!layout || !customer) return;
		choices.push({ kind: layout.kinds[0], customer_id: customer.id, layout_id: layout.id });
	}

	async function saveChoices() {
		const result = await call(api.PUT('/api/layout-choices', { body: choices }));
		error = result.error;
		if (result.data) {
			choices = result.data;
			toast(t('app.saved'));
		}
	}

	async function approve(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		const result = await call(api.POST('/api/packages', { body: { spec } }));
		busy = false;
		packageError = result.error;
		if (result.data) {
			spec = '';
			toast(t('layouts.approved'));
			await load();
		}
	}

	const sample = (id: number, kind: Kind, language: 'de' | 'en') =>
		`/api/layouts/${id}/sample?kind=${kind}&language=${language}`;
</script>

<div class="page-head">
	<div>
		<h1>{t('layouts.title')}</h1>
		<p class="muted">{t('layouts.intro')}</p>
	</div>
	<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('layouts.new')}</button>
</div>

<div class="stack">
	{#if error && !open}<p class="alert" role="alert">{t(error.message)}</p>{/if}

	<ul class="list card-list">
		<li>
			<div>
				<strong>{t('layouts.built_in')}</strong>
				<small class="muted">{t('layouts.built_in_hint')}</small>
			</div>
			<a class="button ghost" href={sample(0, 'invoice', 'de')} target="_blank" rel="noopener">
				<Icon name="pdf" size={18} />{t('layouts.sample')}
			</a>
		</li>
		{#each layouts as layout (layout.id)}
			<li>
				<div>
					<strong>{layout.name}</strong>
					<small class="muted">
						{layout.kinds.map(kindLabel).join(' · ')}
						{#if layout.files.length}· {layout.files.map((f) => f.name).join(', ')}{/if}
						{#if layout.packages.length}· {layout.packages.join(', ')}{/if}
					</small>
					{#if !layout.current}<small class="badge">{t('layouts.outdated')}</small>{/if}
				</div>
				<a
					class="button ghost"
					href={sample(layout.id, layout.kinds[0], 'de')}
					target="_blank"
					rel="noopener"
				>
					<Icon name="pdf" size={18} />{t('layouts.sample')}
				</a>
				<button class="ghost" onclick={() => edit(layout)}>{t('app.edit')}</button>
				<button class="ghost" onclick={() => remove(layout)}>{t('app.delete')}</button>
			</li>
		{/each}
	</ul>

	<section class="card stack">
		<h2>{t('layouts.choices')}</h2>
		<div class="choices">
			{#each kinds as kind (kind)}
				<Field label={kindLabel(kind)}>
					<select value={defaultOf(kind)} onchange={(e) => setDefault(kind, +e.currentTarget.value)}>
						<option value={0}>{t('layouts.built_in')}</option>
						{#each fitting(kind) as layout (layout.id)}
							<option value={layout.id}>{layout.name}</option>
						{/each}
					</select>
				</Field>
			{/each}
		</div>
		<h3>{t('layouts.overrides')}</h3>
		{#if overrides.length}
			{#each overrides as choice, i (i)}
				<div class="override">
					<select aria-label={t('term.customer')} bind:value={choice.customer_id}>
						{#each customers as customer (customer.id)}
							<option value={customer.id}>{customer.name}</option>
						{/each}
					</select>
					<select aria-label={t('layouts.kind')} bind:value={choice.kind}>
						{#each kinds as kind (kind)}<option value={kind}>{kindLabel(kind)}</option>{/each}
					</select>
					<select aria-label={t('term.layout')} bind:value={choice.layout_id}>
						{#each fitting(choice.kind) as layout (layout.id)}
							<option value={layout.id}>{layout.name}</option>
						{/each}
					</select>
					<button
						class="ghost icon"
						aria-label={t('app.remove')}
						onclick={() => (choices = choices.filter((c) => c !== choice))}
					>
						<Icon name="close" size={18} />
					</button>
				</div>
			{/each}
		{:else}
			<p class="muted">{t('layouts.no_overrides')}</p>
		{/if}
		<div class="actions">
			<button class="ghost" onclick={addOverride} disabled={!layouts.length || !customers.length}>
				<Icon name="plus" size={18} />{t('layouts.add_override')}
			</button>
			<button class="primary" onclick={saveChoices}>{t('app.save')}</button>
		</div>
	</section>

	<section class="card stack">
		<h2>{t('layouts.packages')}</h2>
		<p class="muted">{t('layouts.packages_hint')}</p>
		{#if packages.length}
			<ul class="plain">
				{#each packages as pkg (pkg.spec)}
					<li>
						<code>{pkg.spec}</code>
						<small class="muted">{pkg.approved_by}</small>
						{#if pkg.requires.length}
							<small class="badge">{t('layouts.requires', { packages: pkg.requires.join(', ') })}</small>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
		<form class="approve" onsubmit={approve}>
			<Field label={t('layouts.package')} error={packageError?.fields?.spec}>
				<input bind:value={spec} placeholder="@preview/name:1.0.0" />
			</Field>
			<button class="primary" disabled={busy}>{t('layouts.approve')}</button>
		</form>
		{#if packageError && !packageError.fields}<p class="alert" role="alert">{t(packageError.message)}</p>{/if}
	</section>
</div>

<Dialog bind:open title={t(editing ? 'layouts.edit' : 'layouts.new')}>
	<form class="stack" onsubmit={save}>
		<Field label={t('layouts.name')} error={uploadError?.fields?.name}>
			<input bind:value={name} />
		</Field>
		<fieldset>
			<legend>{t('layouts.kinds')}</legend>
			{#each kinds as kind (kind)}
				<label class="check">
					<input type="checkbox" value={kind} bind:group={chosenKinds} />{kindLabel(kind)}
				</label>
			{/each}
			{#if uploadError?.fields?.kinds}
				<small class="error">{t(`field.${uploadError.fields.kinds}`)}</small>
			{/if}
		</fieldset>
		<Field
			label={t('layouts.source')}
			error={uploadError?.fields?.source}
			note={editing ? t('layouts.source_keep') : undefined}
		>
			<input type="file" accept=".typ" bind:files={source} />
		</Field>
		<Field
			label={t('layouts.files')}
			error={uploadError?.fields?.files}
			note={t(editing ? 'layouts.files_keep' : 'layouts.files_hint')}
		>
			<input type="file" multiple bind:files />
		</Field>
		{#if uploadError}
			<div class="alert" role="alert">
				{t(uploadError.message)}
				{#if uploadError.detail}<pre>{uploadError.detail}</pre>{/if}
			</div>
		{/if}
		<div><button class="primary" disabled={busy}>{t(busy ? 'layouts.checking' : 'app.save')}</button></div>
	</form>
</Dialog>

<style>
	.card-list {
		max-width: 56rem;
		background: var(--surface);
	}
	section {
		max-width: 56rem;
	}
	h2,
	h3 {
		margin: 0;
	}
	h3 {
		font-size: 1rem;
	}
	.choices {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: 1rem;
	}
	.override {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr auto;
		gap: 0.5rem;
	}
	@media (max-width: 40rem) {
		.override {
			grid-template-columns: 1fr auto;
		}
	}
	.actions,
	.approve {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: end;
	}
	.approve :global(label) {
		flex: 1 1 14rem;
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 1rem;
	}
	legend {
		margin-bottom: 0.375rem;
		font-weight: 500;
	}
	.check {
		display: flex;
		gap: 0.375rem;
		align-items: center;
	}
	.plain {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 0.5rem;
	}
	.plain li {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		align-items: center;
	}
	pre {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		font-size: 0.75rem;
		margin: 0.5rem 0 0;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
</style>
