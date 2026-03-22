import { parseGitLabUrl } from '@/utils/url-parser';
import { generateFaviconSync } from '@/utils/favicon-generator';

export default defineContentScript({
  matches: ['*://*.gitlab.com/*'],
  runAt: 'document_end',

  main(ctx) {
    // Apply favicon for the current URL
    applyFavicon(window.location.pathname);

    // GitLab is a SPA — listen for URL changes
    ctx.addEventListener(window, 'wxt:locationchange', ({ newUrl }) => {
      const url = new URL(newUrl);
      applyFavicon(url.pathname);
    });
  },
});

function applyFavicon(pathname: string): void {
  const resource = parseGitLabUrl(pathname);
  const dataUrl = generateFaviconSync(resource);

  if (!dataUrl) return;

  // Find existing favicon links
  const existingLinks = document.querySelectorAll<HTMLLinkElement>(
    'link[rel="icon"], link[rel="shortcut icon"]',
  );

  if (existingLinks.length > 0) {
    for (const link of existingLinks) {
      link.href = dataUrl;
      // Remove size constraints so our icon is preferred
      link.removeAttribute('sizes');
      link.type = 'image/png';
    }
  } else {
    // Create a new favicon link
    const link = document.createElement('link');
    link.rel = 'icon';
    link.type = 'image/png';
    link.href = dataUrl;
    document.head.appendChild(link);
  }
}
