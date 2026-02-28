import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { SWRConfig } from 'swr'
import './index.css'
import ArbDashboard from './ArbDashboard'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <SWRConfig value={{ revalidateOnFocus: true, revalidateOnReconnect: true }}>
      <ArbDashboard />
    </SWRConfig>
  </StrictMode>,
)
