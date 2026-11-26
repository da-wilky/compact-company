<script lang="ts">
	import type { Schemas } from '#lib/api/index.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';

	type Account = Schemas['Account'];

	let {
		accounts,
		value = $bindable(),
		label,
		placeholder,
		class: className
	}: {
		accounts: Account[];
		value: number | null | undefined;
		label: string;
		placeholder?: string;
		class?: string;
	} = $props();

	// accounts the posting rules book on by themselves; simple books never offer them
	const automatic = /^(input_vat|vat|vat_pending|advances|receivables|payables|depreciation|low_value_write_off|asset_disposal|fx_)/;
	const simple = $derived(session.view?.simple_books ?? false);
	const offered = $derived(simple ? accounts.filter((a) => !automatic.test(a.role ?? '') || a.id === value) : accounts);

	const groupOf = (a: Account) => a.euer_line ?? a.type;
	const rank = (key: string) => (/^\d/.test(key) ? parseFloat(key) : 1000 + ['asset', 'equity', 'liability', 'income', 'expense'].indexOf(key));
	const groups = $derived.by(() => {
		const byKey = new Map<string, Account[]>();
		for (const account of offered) byKey.set(groupOf(account), [...(byKey.get(groupOf(account)) ?? []), account]);
		return [...byKey.entries()]
			.sort(([a], [b]) => rank(a) - rank(b))
			.map(([key, list]) => ({
				label: /^\d/.test(key) ? t(`tax.euer.${key}`) : t(`simple.group.${key}`),
				accounts: list.sort((a, b) => a.name.localeCompare(b.name))
			}));
	});
</script>

<select class={className} aria-label={label} bind:value>
	{#if placeholder}<option value={null}>{placeholder}</option>{/if}
	{#if !simple}
		{#each offered as account (account.id)}<option value={account.id}>{account.number} {account.name}</option>{/each}
	{:else if groups.length === 1}
		{#each groups[0].accounts as account (account.id)}<option value={account.id}>{account.name}</option>{/each}
	{:else}
		{#each groups as group (group.label)}
			<optgroup label={group.label}>
				{#each group.accounts as account (account.id)}<option value={account.id}>{account.name}</option>{/each}
			</optgroup>
		{/each}
	{/if}
</select>
