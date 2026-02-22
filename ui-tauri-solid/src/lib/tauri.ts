import { invoke } from "@tauri-apps/api/core";

type Method = "GET" | "POST" | "PATCH" | "PUT" | "DELETE";

interface CoreRequestInput {
  method: Method;
  path: string;
  body?: unknown;
  adminToken?: string;
}

interface CoreResponse {
  status: number;
  body: unknown;
}

function messageFromBody(status: number, body: unknown): string {
  if (typeof body === "string" && body.trim().length > 0) {
    return body;
  }
  if (body && typeof body === "object") {
    const maybeError = (body as { error?: unknown }).error;
    if (typeof maybeError === "string" && maybeError.trim().length > 0) {
      return maybeError;
    }
  }
  return `Request failed (${status})`;
}

export async function coreRequest<T>(input: CoreRequestInput): Promise<T> {
  const response = await invoke<CoreResponse>("core_request", {
    input: {
      method: input.method,
      path: input.path,
      body: input.body ?? null,
      adminToken: input.adminToken?.trim() || null
    }
  });

  if (response.status >= 400) {
    throw new Error(messageFromBody(response.status, response.body));
  }

  return response.body as T;
}
