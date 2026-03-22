import { defineConfig } from 'wxt';

export default defineConfig({
  srcDir: 'src',
  outDir: 'build',
  manifest: {
    name: 'giTab',
    description: 'Custom favicons and resource labels for GitLab tabs',
    permissions: ['activeTab', 'storage', 'scripting'],
    host_permissions: ['*://*.gitlab.com/*'],
    optional_host_permissions: ['*://*/*'],
    browser_specific_settings: {
      gecko: { id: 'gitab@gar.im' },
    },
  },
});
