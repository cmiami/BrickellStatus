import { redirect } from '@sveltejs/kit';

// Legacy alert-policy links now land on the one global delivery desk.
export function load() {
  redirect(307, '/system/outputs');
}
