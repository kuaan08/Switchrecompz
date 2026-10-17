/* SwitchRecomp by MZ Labs; fork additions, Apache-2.0. */
#ifndef SWR_ANGLE_WIN32_H
#define SWR_ANGLE_WIN32_H
#include <windows.h>
int swr_angle_create(HWND window, const char* renderer, int vsync);
int swr_angle_active(void);
void* swr_angle_proc(const char* name);
int swr_angle_swap(void);
int swr_angle_worker_current(void);
const char* swr_angle_error(void);
#endif
