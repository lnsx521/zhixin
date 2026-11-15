import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import Home from './Home.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('知信Home', Home)
  },
} satisfies Theme
