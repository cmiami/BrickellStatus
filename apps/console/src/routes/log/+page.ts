import { redirect } from '@sveltejs/kit';

// The log moved under System for the same reason Outputs did: neither is a
// place you go to decide something, which is what the rail is for.
export function load() {
  redirect(307, '/system/log');
}
