import createClient from 'openapi-fetch';
import type { components, paths } from './schema.d.ts';

export type Schemas = components['schemas'];
export type ApiError = Schemas['ApiError'];

export const api = createClient<paths>();

let csrfToken: string | null = null;

export function setCsrfToken(token: string | null | undefined) {
	csrfToken = token ?? null;
}

api.use({
	onRequest({ request }) {
		if (csrfToken && !['GET', 'HEAD'].includes(request.method)) {
			request.headers.set('x-csrf-token', csrfToken);
		}
	}
});

/** An error raised in the browser, shown like the server's errors. */
export const clientError = (message: string): ApiError => ({ code: 'internal', message });

/** Runs an API call, turning a failed fetch into an error to show. */
export async function call<T>(
	request: Promise<{ data?: T; error?: ApiError }>
): Promise<{ data?: T; error?: ApiError }> {
	try {
		return await request;
	} catch {
		return { error: clientError('error.network') };
	}
}
