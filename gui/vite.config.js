import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [tailwindcss()],
  clearScreen: false,
  server: { strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
});
