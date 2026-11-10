<script lang="ts">
	import { i18n, t, type Locale } from '#lib/i18n.svelte.ts';
	import { preferences, setLocale, setTheme, type Theme } from '#lib/preferences.svelte.ts';
	import Segmented from './Segmented.svelte';

	const themes: Theme[] = ['system', 'light', 'dark'];
	const locales: Locale[] = ['de', 'en'];
</script>

<div class="row">
	<Segmented
		label={t('appearance.theme')}
		options={themes.map((value) => ({ value, label: t(`theme.${value}`) }))}
		value={preferences.theme}
		onchange={setTheme}
	/>
	<Segmented
		label={t('appearance.language')}
		options={locales.map((value) => ({ value, label: value.toUpperCase() }))}
		value={i18n.locale}
		onchange={setLocale}
	/>
</div>
