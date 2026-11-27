import { api, call, type ApiError } from './api/index.ts';

/** Sends the browser to the identity provider; returns only if that could not start. */
export async function startOidc(invitation?: string): Promise<ApiError | undefined> {
	const { data, error } = await call(api.POST('/api/oidc/start', { body: { invitation } }));
	if (data) location.assign(data.url);
	return error;
}

/** Sends the browser to the identity provider to link it to the signed-in user. */
export async function linkOidc(): Promise<ApiError | undefined> {
	const { data, error } = await call(api.POST('/api/oidc/link'));
	if (data) location.assign(data.url);
	return error;
}
