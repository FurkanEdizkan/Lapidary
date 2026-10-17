import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";
import { strings } from "../lib/strings";
import { SetupPage } from "./setup";

afterEach(() => vi.unstubAllGlobals());

const STATUS = {
  required: true,
  suggestedName: "Furkan Edizkan",
  hostIngestRoot: "/srv/models",
  hostInboxRoot: null,
  hostStorageRoot: "/srv/lapidary",
  importLinksForcedOff: false,
};

const FOLDER = {
  path: "/srv/models",
  store: "/srv/lapidary",
  lockedBy: null,
  canChoose: true,
};

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function renderSetup() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <SetupPage />
    </QueryClientProvider>,
  );
}

/** A field by its label's own first words: a label holds its hint as well. */
function type(label: string, value: string) {
  const holder = [...document.querySelectorAll("label")].find(
    (l) => l.firstChild?.nodeValue === label,
  );
  const input = holder?.querySelector("input");
  if (input == null) throw new Error(`no field labelled ${label}`);
  fireEvent.change(input, { target: { value } });
}

test("the wizard checks the account, shows the server, and a wrong code sends it back to step one", async () => {
  const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === "/api/setup" && init?.method === "POST") {
      return json(403, {
        code: "wrong_code",
        message: "That setup code is not the one this server printed.",
      });
    }
    if (url === "/api/setup/storage") return json(200, FOLDER);
    if (url.startsWith("/api/setup/storage/folders")) {
      const path =
        new URL(url, "http://lapidary.test").searchParams.get("path") ??
        "/srv/models";
      return json(200, {
        path,
        parent: path === "/" ? null : "/",
        folders: path === "/srv/models" ? ["printables"] : [],
        choosable: true,
        problem: null,
      });
    }
    return json(200, STATUS);
  });
  vi.stubGlobal("fetch", fetchMock);
  renderSetup();

  // The name this install already used is offered.
  expect(await screen.findByDisplayValue("Furkan Edizkan")).toBeTruthy();
  type(strings.setup.codeLabel, "K7QF-M2XD-PR4T");
  type(strings.setup.usernameLabel, "furkan");
  type(strings.setup.passwordLabel, "correct horse battery");
  type(strings.setup.confirmLabel, "correct horse batterz");
  fireEvent.click(screen.getByRole("button", { name: strings.setup.next }));
  expect(screen.getByRole("alert").textContent).toBe(
    strings.setup.passwordsDiffer,
  );

  type(strings.setup.confirmLabel, "correct horse battery");
  fireEvent.click(screen.getByRole("button", { name: strings.setup.next }));
  expect(await screen.findByText("/srv/lapidary")).toBeTruthy();
  expect(screen.getByText("/srv/models")).toBeTruthy();
  // The folder picker, before anybody can sign in, asks with the setup code.
  fireEvent.click(
    await screen.findByRole("button", { name: strings.folderPicker.choose }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: strings.folderPicker.open("printables"),
    }),
  );
  expect(await screen.findByText("/srv/models/printables")).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: strings.folderPicker.use }),
  );
  expect(screen.getByText("/srv/models/printables")).toBeTruthy();
  const listed = fetchMock.mock.calls.find(([url]) =>
    String(url).startsWith("/api/setup/storage/folders"),
  );
  expect(new Headers(listed?.[1]?.headers).get("x-setup-code")).toBe(
    "K7QF-M2XD-PR4T",
  );

  fireEvent.click(screen.getByRole("button", { name: strings.setup.next }));
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: strings.setup.finish }));

  expect(
    await screen.findByText(
      "That setup code is not the one this server printed.",
    ),
  ).toBeTruthy();
  expect(screen.getByText(strings.setup.step(1, 3))).toBeTruthy();
  const post = fetchMock.mock.calls.find(([, init]) => init?.method === "POST");
  expect(JSON.parse(String(post?.[1]?.body))).toEqual({
    code: "K7QF-M2XD-PR4T",
    username: "furkan",
    displayName: "Furkan Edizkan",
    password: "correct horse battery",
    importLinks: false,
  });
});
