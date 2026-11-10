import { clientError, type ApiError } from './api/index.ts';

type Result = { credential: unknown; error?: undefined } | { credential?: undefined; error: ApiError };

/** Creates a passkey from the server's registration options. */
export async function createPasskey(options: { [key: string]: unknown }): Promise<Result> {
	const publicKey = PublicKeyCredential.parseCreationOptionsFromJSON(
		options.publicKey as PublicKeyCredentialCreationOptionsJSON
	);
	// sign-in picks the passkey without a user name, so it must be discoverable
	publicKey.authenticatorSelection = {
		...publicKey.authenticatorSelection,
		residentKey: 'required',
		requireResidentKey: true
	};
	return prompt(() => navigator.credentials.create({ publicKey }));
}

/** Lets the user pick one of their passkeys for the server's sign-in options. */
export async function usePasskey(options: { [key: string]: unknown }): Promise<Result> {
	const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(
		options.publicKey as PublicKeyCredentialRequestOptionsJSON
	);
	return prompt(() => navigator.credentials.get({ publicKey }));
}

async function prompt(ask: () => Promise<Credential | null>): Promise<Result> {
	try {
		const credential = (await ask()) as PublicKeyCredential | null;
		if (credential) return { credential: credential.toJSON() };
	} catch {
		// the browser reports cancellation and timeout alike
	}
	return { error: clientError('error.passkey_cancelled') };
}
