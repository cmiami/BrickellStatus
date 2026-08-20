import { redirect } from '@sveltejs/kit';

// Policy moved under Channels, the way Log and Outputs sit under System: it is
// a second desk answering "what reaches me", not a destination of its own.
export function load() {
  redirect(307, '/channels/policy');
}
