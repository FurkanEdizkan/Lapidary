import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider, createRouter } from '@tanstack/react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { routeTree } from './routeTree.gen'
import { Crash } from './components/Crash'
import './styles.css'

// One boundary for every route. Per-route `errorComponent`s would each need a reason to
// differ, and none of them has one: every failure that reaches here is a bug in our render,
// and the page says the same true thing about all of them.
const router = createRouter({ routeTree, defaultErrorComponent: Crash })
const queryClient = new QueryClient()

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('index.html is missing its #root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
)
