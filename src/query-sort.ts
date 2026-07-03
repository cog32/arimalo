export type QueryWindowSort = {
  column?: string;
  direction?: "asc" | "desc";
};

export type BackendTransactionSort = {
  field: "date" | "amount" | "payee";
  order: "asc" | "desc";
};

/** Backend-supported columns should be sorted by Rust before pagination.
 * Unsupported columns intentionally keep a stable date-desc backend order and
 * are only sorted locally within the loaded transaction window. */
export function backendSortForTransactionWindow(sort?: QueryWindowSort): BackendTransactionSort {
  switch (sort?.column) {
    case "amount":
      return { field: "amount", order: sort.direction ?? "desc" };
    case "party":
      return { field: "payee", order: sort.direction ?? "desc" };
    case "notes":
    case "category":
      return { field: "date", order: "desc" };
    case "date":
      return { field: "date", order: sort.direction ?? "desc" };
    default:
      return { field: "date", order: "desc" };
  }
}

export function shouldClientSortLoadedTransactions(sort?: QueryWindowSort): boolean {
  return sort?.column === "notes" || sort?.column === "category";
}
