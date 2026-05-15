export function doctorRepairsForStatus(status) {
  const repairs = [];
  for (const change of status.c4?.repairChanges ?? []) {
    repairs.push({
      id: `c4:${change}`,
      category: "c4",
      file: "docs/architext/data/views.json",
      summary: change
    });
  }
  return repairs;
}

export function doctorRepairCategories(repairs) {
  return [...new Set(repairs.map((repair) => repair.category))];
}
