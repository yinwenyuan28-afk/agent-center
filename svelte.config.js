import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** Svelte 配置。vitePreprocess 让组件里的 <script lang="ts"> 走 TypeScript 处理。 */
export default {
  preprocess: vitePreprocess(),
};
