# Three fixed roles

A user is owner, staff or accountant. Staff work on todos and their own time and see customers and projects; the accountant, usually the tax advisor, reads the books and takes exports but changes nothing; the owner does everything. This revises ADR 0003, which kept roles out of scope: the accountant needs access without being able to issue or book. A permission editor was rejected as more UI and testing than three roles justify.
