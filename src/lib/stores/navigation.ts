import { writable } from 'svelte/store';

export type Tab = "sessions" | "similarity" | "devices" | "settings";

export const activeTab = writable<Tab>("sessions");
