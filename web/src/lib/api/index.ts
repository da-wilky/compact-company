import createClient from 'openapi-fetch';
import type { components, paths } from './schema.d.ts';

export type Schemas = components['schemas'];
export type ApiError = Schemas['ApiError'];

export const api = createClient<paths>();

/** The error to show when the server cannot be reached at all. */
export const networkError: ApiError = { code: 'internal', message: 'error.network' };
