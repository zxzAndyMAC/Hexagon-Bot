-- 0013: 消息图片附件（agent-senses 票 03）。
-- attachments = JSON [{media_type,path,bytes,name}]——字节本体落
-- .hexagon/inbox/ 文件，行里只存引用（base64 进 messages 表会让
-- 时间线查询每次全量驮回）。stage_attachment 先写文件再发消息；
-- 发送失败的孤儿文件由 discard_attachments/周期清扫负责。
ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';
