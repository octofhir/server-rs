import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// Adjust site/base for GitHub Pages deployment under org/repo path
export default defineConfig({
  site: 'https://octofhir.github.io/server-rs',
  base: '/server-rs/',
  integrations: [
    starlight({
      title: 'OctoFHIR',
      description: 'Core FHIR server of the OctoFHIR ecosystem, built in Rust',
      social: [
          { label: 'Storybook', icon: 'external', href: 'https://octofhir.github.io/server-rs/storybook/'},
          { label: 'Boosty', icon: 'external', href: 'https://boosty.to/octoshikari'},
          { label: 'Github', icon: 'github', href: 'https://github.com/octofhir/server-rs'},
      ],
      logo: {
        light: './src/assets/logo-light.svg',
        dark: './src/assets/logo-dark.svg',
        alt: 'OctoFHIR',
        replacesTitle: true
      },
      customCss: ['./src/styles/custom.css'],
      head: [
        { tag: 'link', attrs: { rel: 'icon', type: 'image/svg+xml', href: 'favicon.svg' } },
        { tag: 'link', attrs: { rel: 'icon', href: 'favicon.ico', sizes: '32x32' } },
        { tag: 'link', attrs: { rel: 'icon', type: 'image/png', sizes: '32x32', href: 'favicon-32x32.png' } },
        { tag: 'link', attrs: { rel: 'icon', type: 'image/png', sizes: '16x16', href: 'favicon-16x16.png' } },
        { tag: 'link', attrs: { rel: 'apple-touch-icon', sizes: '180x180', href: 'apple-touch-icon.png' } },
        { tag: 'link', attrs: { rel: 'manifest', href: 'site.webmanifest' } },
        { tag: 'link', attrs: { rel: 'mask-icon', color: '#f2b630', href: 'safari-pinned-tab.svg' } },
        { tag: 'meta', attrs: { name: 'theme-color', content: '#111214' } }
      ],
      sidebar: [
        { label: 'Overview', link: '' },
        { label: 'Getting Started', link: 'getting-started/' },
        { label: 'Configuration', link: 'configuration/' },
        {
          label: 'Authentication',
          collapsed: false,
          items: [
            { label: 'Overview', link: 'authentication/' },
            { label: 'Auth Architecture', link: 'auth-architecture/' },
            { label: 'Access Policies', link: 'access-policies/' },
            { label: 'JWT Key Persistence', link: 'jwt-key-persistence/' },
          ]
        },
        { label: 'Security', link: 'security/' },
        { label: 'API Reference', link: 'api-reference/' },
        { label: 'GraphQL', link: 'graphql/' },
        {
          label: 'Search',
          collapsed: false,
          items: [
            { label: 'Search Parameters', link: 'search-parameters/' },
            { label: 'Search Indexing', link: 'search-indexing/' },
            { label: 'Search Architecture', link: 'search-architecture/' },
            { label: 'Date Search', link: 'date-search/' },
          ]
        },
        {
          label: 'Features',
          collapsed: false,
          items: [
            { label: 'Transactions', link: 'transactions/' },
            { label: 'Bulk Data Export', link: 'bulk-export/' },
            { label: 'Terminology', link: 'terminology/' },
            { label: 'Notifications', link: 'notifications/' },
          ]
        },
        {
          label: 'Architecture',
          collapsed: false,
          items: [
            { label: 'Event System', link: 'event-system/' },
            { label: 'Performance', link: 'performance/' },
          ]
        },
        { label: 'Application Platform', link: 'applications/' },
        {
          label: 'Migration',
          collapsed: true,
          items: [
            { label: 'From HAPI FHIR', link: 'migration/hapi/' },
          ]
        },
        { label: 'Development', link: 'development/' },
        { label: 'Deployment', link: 'deployment/' },
      ],
    })
  ]
});
