import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

// Tauri 在开发时用固定端口，构建时输出到 dist 供 Rust 端嵌入
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 5731,
    strictPort: true,
    watch: {
      // 必须排除 Rust 编译产物。cargo 写 target/ 时文件句柄被占用，
      // Vite 的文件监听会抛 EBUSY 直接把开发服务器打崩。
      ignored: ['**/src-tauri/target/**', '**/src-tauri/gen/**'],
    },
  },
  build: {
    // Tauri 的 WebView2 支持较新的语法，不需要降级到 es5
    target: 'chrome105',
    outDir: 'dist',
    emptyOutDir: true,
  },
});
