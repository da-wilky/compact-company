/** The app's name wherever people read it; `compact-company` stays the name of the code. */
export const APP_NAME = 'Compact Company';

/** A page's own name, such as a document's number; other pages are named by their route. */
export const title = $state<{ page?: string }>({});

/** Names the current page while it shows. */
export function namePage(name: () => string | undefined) {
	$effect(() => {
		title.page = name();
		return () => (title.page = undefined);
	});
}

export function tabTitle(page?: string): string {
	return page ? `${page} · ${APP_NAME}` : APP_NAME;
}
