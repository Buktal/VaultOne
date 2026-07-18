// Typed Redux hooks (ADR-0007).

import type { TypedUseSelectorHook } from "react-redux"
import { useDispatch, useSelector } from "react-redux"

import type { AppDispatch, RootState } from "./store"

export const useAppDispatch = () => useDispatch<AppDispatch>()
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector
export type { RootState }
