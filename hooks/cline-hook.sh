#!/bin/bash
# Cline hook — place this file (or symlink it) at:
#   ~/Documents/Cline/Rules/Hooks/TaskCancel
#
# Cline fires this hook when the user cancels a task or the task
# errors out. (TaskComplete is not supported yet.)
exec zestful hook --agent cline
