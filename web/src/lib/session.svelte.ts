import { api, call, setCsrfToken, type Schemas } from './api/index.ts';

export const session = $state<{ view?: Schemas['SessionView'] }>({});

export function show(view: Schemas['SessionView']) {
	session.view = view;
	setCsrfToken(view.csrf_token);
}

export async function load() {
	const { data } = await call(api.GET('/api/session'));
	if (data) show(data);
}
