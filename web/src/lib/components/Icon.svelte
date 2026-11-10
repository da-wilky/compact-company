<script lang="ts" module>
	const paths = {
		overview:
			'M3 4.5A1.5 1.5 0 0 1 4.5 3h4A1.5 1.5 0 0 1 10 4.5v4A1.5 1.5 0 0 1 8.5 10h-4A1.5 1.5 0 0 1 3 8.5zM14 4.5A1.5 1.5 0 0 1 15.5 3h4A1.5 1.5 0 0 1 21 4.5v4a1.5 1.5 0 0 1-1.5 1.5h-4A1.5 1.5 0 0 1 14 8.5zM3 15.5A1.5 1.5 0 0 1 4.5 14h4a1.5 1.5 0 0 1 1.5 1.5v4A1.5 1.5 0 0 1 8.5 21h-4A1.5 1.5 0 0 1 3 19.5zM14 15.5a1.5 1.5 0 0 1 1.5-1.5h4a1.5 1.5 0 0 1 1.5 1.5v4a1.5 1.5 0 0 1-1.5 1.5h-4a1.5 1.5 0 0 1-1.5-1.5z',
		business: 'M4 21V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v16M16 9h2a2 2 0 0 1 2 2v10M2 21h20M8 7h4M8 11h4M8 15h4',
		users:
			'M9 11.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2.5 20c.8-3.5 3.4-5.5 6.5-5.5s5.7 2 6.5 5.5M16 4.5a3.5 3.5 0 0 1 0 7M18 14.8c2 .7 3.2 2.5 3.6 5.2',
		audit: 'M3 12a9 9 0 1 0 3-6.7M3 4v4h4M12 7v5l3 2',
		account: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21c.9-4 4-6 8-6s7.1 2 8 6',
		more: 'M5 12h.01M12 12h.01M19 12h.01',
		sign_out: 'M14 4h4.5A1.5 1.5 0 0 1 20 5.5v13a1.5 1.5 0 0 1-1.5 1.5H14M10 16l-4-4 4-4M6 12h10',
		passkey:
			'M10 13a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM3 21c.6-3.4 3.3-5.5 7-5.5 1 0 2 .2 2.8.5M18 13v8M18 21l2-1.5M18 18h2M16 13a2 2 0 1 0 4 0 2 2 0 0 0-4 0',
		copy: 'M9 9h10a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H11a2 2 0 0 1-2-2zM5 15a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2',
		close: 'M6 6l12 12M18 6L6 18',
		check: 'M5 12.5l4.5 4.5L19 7.5'
	};
	export type IconName = keyof typeof paths;
</script>

<script lang="ts">
	let { name, size = 20 }: { name: IconName; size?: number } = $props();
</script>

<svg
	aria-hidden="true"
	width={size}
	height={size}
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width={name === 'more' ? 3 : 1.8}
	stroke-linecap="round"
	stroke-linejoin="round"><path d={paths[name]} /></svg
>
