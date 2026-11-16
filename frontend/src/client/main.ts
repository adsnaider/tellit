import { mount } from 'svelte';

import App from './App.svelte';
import './styles/global.css';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error('Root element not found');
}

mount(App, {
  target: rootElement,
});
