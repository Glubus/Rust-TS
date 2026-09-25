import(__MODULE_ID__).then(async (exportsObject) => {
  const fnName = __FUNCTION_NAME__;
  const args = __FUNCTION_ARGS__;
  const fnRef = exportsObject && exportsObject[fnName];
  if (typeof fnRef !== "function") {
    throw new Error(`missing function: ${fnName}`);
  }
  const result = await fnRef(...args);
  return JSON.stringify(result === undefined ? null : result);
})
