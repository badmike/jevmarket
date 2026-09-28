import { toast } from 'vue-sonner'

import { api, errorMessage } from '~/api/client'
import { useLiveConfirm } from '~/composables/useLiveConfirm'

const LIVE_MESSAGE =
  'Dry run is off: the next pass places real orders on Polymarket, inside the caps in your configuration.'

/** Start a pass, pause and resume, with the live-money confirmation and a toast for the outcome. */
export function useLoopActions() {
  const { guarded } = useLiveConfirm()
  const busy = ref<'pass' | 'pause' | 'resume' | null>(null)

  const run = async (kind: 'pass' | 'pause' | 'resume', action: () => Promise<unknown>, done: string) => {
    busy.value = kind
    try {
      const result = await action()
      if (result !== undefined) toast.success(done)
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      busy.value = null
    }
  }

  return {
    busy: readonly(busy),
    runPass: () =>
      run(
        'pass',
        () => guarded('Start a live pass?', LIVE_MESSAGE, (confirm) => api.runPass(confirm).then(() => true)),
        'Pass started'
      ),
    pause: () => run('pause', () => api.pause().then(() => true), 'Loop paused'),
    resume: () =>
      run(
        'resume',
        () => guarded('Resume live trading?', LIVE_MESSAGE, (confirm) => api.resume(confirm).then(() => true)),
        'Loop resumed'
      ),
  }
}
