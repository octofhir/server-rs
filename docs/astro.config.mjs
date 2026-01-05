import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// Adjust site/base for GitHub Pages deployment under org/repo path
export default defineConfig({
  site: 'https://octofhir.github.io/server-rs',
  base: '/server-rs/',
  integrations: [
    starlight({
      title: 'Abyxon',
      description: 'Core FHIR server of the OctoFHIR ecosystem, built in Rust',
      social: [
          { label: 'Boosty', icon: 'external', href: 'https://boosty.to/octoshikari'},
          { label: 'Github', icon: 'github', href: 'https://github.com/octofhir/server-rs'},

      ],
      logo: {
        src: './src/assets/logo.png',
        alt: 'Abyxon',
        dark: './src/assets/logo-dark.png'
      },
      customCss: ['./src/styles/custom.css'],
      head: [
        { tag: 'link', attrs: { rel: 'icon', href: 'favicon.ico' } },
        { tag: 'link', attrs: { rel: 'icon', type: 'image/png', sizes: '32x32', href: 'favicon-32x32.png' } },
        { tag: 'link', attrs: { rel: 'icon', type: 'image/png', sizes: '16x16', href: 'favicon-16x16.png' } },
        { tag: 'link', attrs: { rel: 'apple-touch-icon', sizes: '180x180', href: 'apple-touch-icon.png' } },
        { tag: 'link', attrs: { rel: 'manifest', href: 'site.webmanifest' } },
        { tag: 'link', attrs: { rel: 'mask-icon', color: '#0b4d7a', href: 'safari-pinned-tab.svg' } },
        { tag: 'meta', attrs: { name: 'theme-color', content: '#0b4d7a' } },
        { tag: 'meta', attrs: { name: 'msapplication-TileColor', content: '#0b4d7a' } },
        { tag: 'meta', attrs: { name: 'msapplication-TileImage', content: 'mstile-150x150.png' } }
      ],
      sidebar: [
        { label: 'Overview', link: '' },
        { label: 'Getting Started', link: 'getting-started/' },
        {
          label: 'Authentication',
          collapsed: false,
          items: [
            { label: 'Overview', link: 'authentication/' },
            { label: 'Auth Architecture', link: 'auth-architecture/' },
            { label: 'JWT Key Persistence', link: 'jwt-key-persistence/' },
          ]
        },
        { label: 'API Reference', link: 'api-reference/' },
        {
          label: 'Search',
          collapsed: false,
          items: [
            { label: 'Search Parameters', link: 'search-parameters/' },
            { label: 'Search Architecture', link: 'search-architecture/' },
          ]
        },
        {
          label: 'Architecture',
          collapsed: false,
          items: [
            { label: 'Event System', link: 'event-system/' },
          ]
        },
        { label: 'Application Platform', link: 'applications/' },
        { label: 'Notifications', link: 'notifications/' },
        { label: 'Bulk Data Export', link: 'bulk-export/' },
        { label: 'Development', link: 'development/' },
        { label: 'Deployment', link: 'deployment/' },
      ],
    })
  ]
});
