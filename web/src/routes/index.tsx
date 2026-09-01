import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { fetchHealth } from '../lib/api'
import { strings } from '../lib/strings'

export const Route = createFileRoute('/')({ component: Index })

export function Index() {
  const { data, isPending, isError } = useQuery({ queryKey: ['health'], queryFn: fetchHealth })

  return (
    <section>
      <h2 className="text-lg">{strings.emptyLibrary.title}</h2>
      <p className="text-[var(--color-muted)]">{strings.emptyLibrary.body}</p>
      <p className="mt-6 text-sm text-[var(--color-muted)]">
        {isPending
          ? strings.health.checking
          : isError
            ? strings.health.failed
            : strings.health.ok(data.database.major)}
      </p>
    </section>
  )
}
