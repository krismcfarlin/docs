/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    extend: {
      fontFamily: {
        sans: ['Inter', 'ui-sans-serif', 'system-ui', 'sans-serif'],
      },
      colors: {
        sidebar:    'var(--color-sidebar)',
        surface:    'var(--color-surface)',
        panel:      'var(--color-panel)',
        primary:    'var(--color-primary)',
        'primary-dim': 'var(--color-primary-dim)',
        'on-surface':      'var(--color-on-surface)',
        'on-muted':        'var(--color-on-muted)',
        'surface-hi':      'var(--color-surface-hi)',
        'surface-lo':      'var(--color-surface-lo)',
        'surface-lowest':  'var(--color-surface-lowest)',
        bdr:        'var(--color-border)',
        // legacy
        accent:       'var(--color-primary)',
        'accent-hover': 'var(--color-primary-dim)',
      },
      boxShadow: {
        ambient: 'var(--shadow-ambient)',
        card:    'var(--shadow-card)',
      },
    },
  },
  plugins: [],
};
