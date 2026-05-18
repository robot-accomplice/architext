export function createMinHeap() {
  const values = [];
  const swap = (left, right) => {
    const value = values[left];
    values[left] = values[right];
    values[right] = value;
  };
  const push = (item) => {
    values.push(item);
    let index = values.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (values[parent].distance <= values[index].distance) break;
      swap(parent, index);
      index = parent;
    }
  };
  const pop = () => {
    if (values.length === 0) return null;
    const root = values[0];
    const last = values.pop();
    if (values.length > 0) {
      values[0] = last;
      let index = 0;
      while (true) {
        const left = index * 2 + 1;
        const right = left + 1;
        let smallest = index;
        if (left < values.length && values[left].distance < values[smallest].distance) smallest = left;
        if (right < values.length && values[right].distance < values[smallest].distance) smallest = right;
        if (smallest === index) break;
        swap(index, smallest);
        index = smallest;
      }
    }
    return root;
  };
  return {
    get size() {
      return values.length;
    },
    push,
    pop
  };
}
